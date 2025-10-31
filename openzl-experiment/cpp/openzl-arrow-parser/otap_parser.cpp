/**
 * OpenZL Parser for OTAP dataset
 *
 * This parser processes .batch files containing BatchArrowRecords protobuf messages
 * and dispatches Arrow IPC payloads by their ArrowPayloadType for format-aware compression.
 *
 * Uses OpenZL C API for all OpenZL operations.
 */

#include "otap_proto.h"
#include <openzl/openzl.h>
#include <openzl/codecs/zl_clustering.h>

#include <cassert>
#include <vector>
#include <unordered_map>
#include <cstring>

namespace otap {

// Tag IDs for clustering metadata
constexpr unsigned TAG_BATCH_METADATA = 100;  // batch_id + headers
constexpr unsigned TAG_PAYLOAD_OFFSET = 200;  // Base offset for payload types

/**
 * Parsing compressor graph function for OTAP dataset
 *
 * Takes BatchArrowRecords protobuf as input and dispatches each ArrowPayload
 * by its type to separate streams for clustering.
 */
static ZL_Report otapParsingCompressorGraphFn(
        ZL_Graph* graph,
        ZL_Edge* inputEdges[],
        size_t numInputs) noexcept
{
    // Setup error context
    ZL_RESULT_DECLARE_SCOPE_REPORT(graph);

    assert(numInputs == 1);
    const ZL_Input* const input = ZL_Edge_getData(inputEdges[0]);
    const uint8_t* const inputData = (const uint8_t*)ZL_Input_ptr(input);
    const size_t inputSize = ZL_Input_numElts(input);

    // Parse with segment tracking
    BatchArrowRecords batch;
    std::vector<FieldSegment> segments;
    if (!parseBatchWithSegments(inputData, inputSize, batch, segments)) {
        return ZL_returnError(ZL_ErrorCode_corruption);
    }

    if (segments.empty()) {
        // No fields - just pass through
        ZL_ERR_IF_ERR(ZL_Edge_setDestination(inputEdges[0], ZL_GRAPH_COMPRESS_GENERIC));
        return ZL_returnSuccess();
    }

    // Build dispatch mapping
    // Reserve dispatch index 0 for metadata (batch_id, headers)
    const uint32_t METADATA_DISPATCH_IDX = 0;
    std::unordered_map<ArrowPayloadType, uint32_t> typeToDispatchIdx;
    uint32_t currentDispatchIdx = 1;  // Start at 1, 0 is for metadata

    for (const auto& seg : segments) {
        if (seg.fieldType == FieldSegment::PAYLOAD) {
            if (typeToDispatchIdx.find(seg.payloadType) == typeToDispatchIdx.end()) {
                typeToDispatchIdx[seg.payloadType] = currentDispatchIdx++;
            }
        }
    }

    // Build dispatch arrays
    std::vector<size_t> segmentSizes;
    std::vector<unsigned> tags;

    for (const auto& seg : segments) {
        segmentSizes.push_back(seg.size);

        if (seg.fieldType == FieldSegment::PAYLOAD) {
            tags.push_back(typeToDispatchIdx[seg.payloadType]);
        } else {
            // batch_id or headers -> metadata
            tags.push_back(METADATA_DISPATCH_IDX);
        }
    }

    // Create dispatch instructions
    ZL_DispatchInstructions instructions = {
        .segmentSizes = segmentSizes.data(),
        .tags = tags.data(),
        .nbSegments = segments.size(),
        .nbTags = currentDispatchIdx
    };

    // Run dispatch
    ZL_TRY_LET(
        ZL_EdgeList,
        dispatchEdges,
        ZL_Edge_runDispatchNode(inputEdges[0], &instructions));

    // Dispatch produces nbTags + 2 edges:
    // edges[0] = tags metadata
    // edges[1] = sizes metadata
    // edges[2] = dispatch_idx 0 data (metadata: batch_id + headers)
    // edges[3+] = dispatch_idx 1+ data (payloads by type)
    assert(dispatchEdges.nbEdges == currentDispatchIdx + 2);

    // Route metadata edges
    ZL_ERR_IF_ERR(ZL_Edge_setDestination(
        dispatchEdges.edges[0], ZL_GRAPH_COMPRESS_GENERIC));  // tags
    ZL_ERR_IF_ERR(ZL_Edge_setDestination(
        dispatchEdges.edges[1], ZL_GRAPH_COMPRESS_GENERIC));  // sizes

    // Route metadata data edge (dispatch_idx 0)
    ZL_ERR_IF_ERR(ZL_Edge_setDestination(
        dispatchEdges.edges[2], ZL_GRAPH_COMPRESS_GENERIC));  // batch_id + headers

    // Get custom clustering graph
    ZL_GraphIDList customGraphs = ZL_Graph_getCustomGraphs(graph);
    ZL_ERR_IF_NE(customGraphs.nbGraphIDs, 1, graphParameter_invalid);

    // Route payload edges to clustering with type tags
    for (const auto& [payloadType, dispatchIdx] : typeToDispatchIdx) {
        size_t edgeIndex = 2 + dispatchIdx;  // +2 for tags/sizes metadata edges
        uint32_t clusteringTag = TAG_PAYLOAD_OFFSET + static_cast<uint32_t>(payloadType);

        ZL_ERR_IF_ERR(ZL_Edge_setIntMetadata(
            dispatchEdges.edges[edgeIndex],
            ZL_CLUSTERING_TAG_METADATA_ID,
            clusteringTag));

        ZL_ERR_IF_ERR(ZL_Edge_setDestination(
            dispatchEdges.edges[edgeIndex],
            customGraphs.graphids[0]));
    }

    return ZL_returnSuccess();
}

/**
 * Register clustering graph with default configuration
 */
ZL_GraphID registerClusteringGraph(ZL_Compressor* compressor) {
    // Empty default config - let training discover optimal clusters
    ZL_ClusteringConfig defaultConfig{
        .clusters = NULL,
        .nbClusters = 0,
        .typeDefaults = NULL,
        .nbTypeDefaults = 0,
    };

    // Standard successors for compression
    std::vector<ZL_GraphID> successors = {
        ZL_GRAPH_STORE,              // No compression
        ZL_GRAPH_ZSTD,               // zstd compression
        ZL_GRAPH_COMPRESS_GENERIC,   // Generic compression
        ZL_Compressor_registerStaticGraph_fromNode1o(
            compressor,
            ZL_NODE_DELTA_INT,
            ZL_GRAPH_FIELD_LZ),      // Delta encoding + LZ
    };

    // Create clustering graph
    ZL_GraphID clusteringGraph = ZL_Clustering_registerGraph(
        compressor,
        &defaultConfig,
        successors.data(),
        successors.size());

    if (!ZL_GraphID_isValid(clusteringGraph)) {
        fprintf(stderr, "Error: Failed to register clustering graph\n");
    }

    return clusteringGraph;
}

/**
 * Register the OTAP parsing compressor graph
 */
ZL_GraphID registerOtapParsingGraph(ZL_Compressor* compressor, ZL_GraphID clusteringGraph) {
    // Verify clustering graph is valid
    if (!ZL_GraphID_isValid(clusteringGraph)) {
        fprintf(stderr, "Error: Invalid clustering graph ID\n");
        return ZL_GraphID{0};
    }

    // Step 1: Register base function graph
    ZL_Type inputTypeMask = ZL_Type_serial;
    ZL_FunctionGraphDesc parsingCompressor = {
        .name = "!OTAP Parsing Compressor",
        .graph_f = otapParsingCompressorGraphFn,
        .inputTypeMasks = &inputTypeMask,
        .nbInputs = 1,
        .customGraphs = NULL,
        .nbCustomGraphs = 0,
        .localParams = {},
    };

    ZL_GraphID parsingGraph = ZL_Compressor_registerFunctionGraph(
        compressor,
        &parsingCompressor);

    if (!ZL_GraphID_isValid(parsingGraph)) {
        fprintf(stderr, "Error: Failed to register parsing function graph\n");
        return ZL_GraphID{0};
    }

    // Step 2: Parameterize with clustering graph
    std::vector<ZL_GraphID> customGraphs = { clusteringGraph };
    ZL_GraphParameters params = {
        .name = NULL,  // Use default name
        .customGraphs = customGraphs.data(),
        .nbCustomGraphs = customGraphs.size(),
        .customNodes = NULL,
        .nbCustomNodes = 0,
        .localParams = NULL,
    };

    ZL_RESULT_OF(ZL_GraphID) result = ZL_Compressor_parameterizeGraph(
        compressor,
        parsingGraph,
        &params);

    if (ZL_RES_isError(result)) {
        fprintf(stderr, "Error parameterizing graph: %d\n", (int)ZL_RES_code(result));
        return ZL_GraphID{0};
    }

    ZL_GraphID parameterizedGraph = ZL_RES_value(result);
    if (!ZL_GraphID_isValid(parameterizedGraph)) {
        fprintf(stderr, "Error: Parameterized graph is invalid\n");
        return ZL_GraphID{0};
    }

    return parameterizedGraph;
}

/**
 * Register complete OTAP compression graph (clustering + parser)
 * Returns the parameterized parsing graph ID to use as starting graph
 */
ZL_GraphID registerOtapCompressionGraph(ZL_Compressor* compressor) {
    ZL_GraphID clusteringGraph = registerClusteringGraph(compressor);
    return registerOtapParsingGraph(compressor, clusteringGraph);
}

} // namespace otap

// ============================================================================
// C API Wrappers for FFI
// ============================================================================

extern "C" {

ZL_GraphID otap_registerOtapCompressionGraph(ZL_Compressor* compressor) {
    return otap::registerOtapCompressionGraph(compressor);
}

} // extern "C"
