/**
 * Simplified protobuf structures for OTAP dataset
 * Hand-coded minimal definitions needed for parsing .batch files
 *
 * Based on opentelemetry/proto/experimental/arrow/v1/arrow_service.proto
 */

#ifndef OTAP_PROTO_H
#define OTAP_PROTO_H

#include <cstdint>
#include <string>
#include <vector>

namespace otap {

// ArrowPayloadType enum matching proto definition
enum class ArrowPayloadType : int32_t {
    UNKNOWN = 0,
    RESOURCE_ATTRS = 1,
    SCOPE_ATTRS = 2,
    UNIVARIATE_METRICS = 10,
    NUMBER_DATA_POINTS = 11,
    SUMMARY_DATA_POINTS = 12,
    HISTOGRAM_DATA_POINTS = 13,
    EXP_HISTOGRAM_DATA_POINTS = 14,
    NUMBER_DP_ATTRS = 15,
    SUMMARY_DP_ATTRS = 16,
    HISTOGRAM_DP_ATTRS = 17,
    EXP_HISTOGRAM_DP_ATTRS = 18,
    NUMBER_DP_EXEMPLARS = 19,
    HISTOGRAM_DP_EXEMPLARS = 20,
    EXP_HISTOGRAM_DP_EXEMPLARS = 21,
    NUMBER_DP_EXEMPLAR_ATTRS = 22,
    HISTOGRAM_DP_EXEMPLAR_ATTRS = 23,
    EXP_HISTOGRAM_DP_EXEMPLAR_ATTRS = 24,
    MULTIVARIATE_METRICS = 25,
    METRIC_ATTRS = 26,
    LOGS = 30,
    LOG_ATTRS = 31,
    SPANS = 40,
    SPAN_ATTRS = 41,
    SPAN_EVENTS = 42,
    SPAN_LINKS = 43,
    SPAN_EVENT_ATTRS = 44,
    SPAN_LINK_ATTRS = 45,
};

struct ArrowPayload {
    std::string schema_id;          // field 1
    ArrowPayloadType type;          // field 2
    std::vector<uint8_t> record;    // field 3 (Arrow IPC bytes)
};

struct BatchArrowRecords {
    int64_t batch_id;                        // field 1
    std::vector<ArrowPayload> arrow_payloads; // field 2
    std::vector<uint8_t> headers;            // field 3
};

// Field segment info for dispatch
struct FieldSegment {
    size_t size;                    // Size in bytes (including tag and data)
    enum FieldType {
        BATCH_ID,
        PAYLOAD,
        HEADERS
    } fieldType;
    ArrowPayloadType payloadType;   // Only valid if fieldType == PAYLOAD
};

// Simple protobuf parser - reads BatchArrowRecords from binary data
bool parseBatchArrowRecords(const uint8_t* data, size_t size, BatchArrowRecords& out);

// Parse BatchArrowRecords and track field segments for dispatch
bool parseBatchWithSegments(
    const uint8_t* data,
    size_t size,
    BatchArrowRecords& out,
    std::vector<FieldSegment>& segments);

// Serialize a single ArrowPayload to protobuf bytes
std::vector<uint8_t> serializeArrowPayload(const ArrowPayload& payload);

} // namespace otap

#endif // OTAP_PROTO_H
