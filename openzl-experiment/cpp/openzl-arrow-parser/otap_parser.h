/**
 * OpenZL Parser for OTAP dataset - Header
 */

#ifndef OTAP_PARSER_H
#define OTAP_PARSER_H

#include <openzl/openzl.h>
#include <openzl/cpp/Compressor.hpp>

namespace otap {

// ============================================================================
// C API (uses ZL_Compressor*)
// ============================================================================

/**
 * Register the complete OTAP compression graph (clustering + parser)
 * This must be called before training or using a trained compressor.
 *
 * @param compressor The OpenZL compressor instance (C API)
 * @return The graph ID to use as starting graph for compression
 */
ZL_GraphID registerOtapCompressionGraph(ZL_Compressor* compressor);

/**
 * Register clustering graph separately (advanced usage, C API)
 */
ZL_GraphID registerClusteringGraph(ZL_Compressor* compressor);

/**
 * Register OTAP parsing graph with custom clustering (advanced usage, C API)
 */
ZL_GraphID registerOtapParsingGraph(ZL_Compressor* compressor, ZL_GraphID clusteringGraph);

// ============================================================================
// C++ API (uses openzl::Compressor)
// ============================================================================

/**
 * Register the complete OTAP compression graph (clustering + parser)
 * C++ wrapper version - uses openzl::Compressor
 */
inline ZL_GraphID registerOtapCompressionGraph(openzl::Compressor& compressor) {
    return registerOtapCompressionGraph(compressor.get());
}

/**
 * Register clustering graph separately (C++ wrapper version)
 */
inline ZL_GraphID registerClusteringGraph(openzl::Compressor& compressor) {
    return registerClusteringGraph(compressor.get());
}

/**
 * Register OTAP parsing graph with custom clustering (C++ wrapper version)
 */
inline ZL_GraphID registerOtapParsingGraph(openzl::Compressor& compressor, ZL_GraphID clusteringGraph) {
    return registerOtapParsingGraph(compressor.get(), clusteringGraph);
}

} // namespace otap

// ============================================================================
// C API for FFI (extern "C" linkage, no name mangling)
// ============================================================================

#ifdef __cplusplus
extern "C" {
#endif

/**
 * C API wrapper for registering OTAP compression graph.
 * This MUST be called before deserializing a trained OTAP compressor.
 *
 * @param compressor The OpenZL compressor instance
 * @return The graph ID to use as starting graph for compression
 */
ZL_GraphID otap_registerOtapCompressionGraph(ZL_Compressor* compressor);

#ifdef __cplusplus
}
#endif

#endif // OTAP_PARSER_H
