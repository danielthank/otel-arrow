/**
 * Simple protobuf parser for BatchArrowRecords
 *
 * This is a minimal hand-coded protobuf parser that only handles
 * the specific structure we need, avoiding the need for full protobuf
 * library dependency.
 */

#include "otap_proto.h"
#include <cstring>
#include <stdexcept>

namespace otap {

// Protobuf wire types
enum WireType {
    VARINT = 0,
    I64 = 1,
    LEN = 2,
    SGROUP = 3,
    EGROUP = 4,
    I32 = 5,
};

class ProtoReader {
public:
    ProtoReader(const uint8_t* data, size_t size)
        : data_(data), size_(size), pos_(0) {}

    bool hasMore() const { return pos_ < size_; }

    size_t currentPosition() const { return pos_; }

    uint64_t readVarint() {
        uint64_t result = 0;
        int shift = 0;
        while (pos_ < size_) {
            uint8_t byte = data_[pos_++];
            result |= (uint64_t)(byte & 0x7F) << shift;
            if ((byte & 0x80) == 0) {
                return result;
            }
            shift += 7;
            if (shift >= 64) {
                throw std::runtime_error("Varint too long");
            }
        }
        throw std::runtime_error("Unexpected end of data in varint");
    }

    uint32_t readTag() {
        return static_cast<uint32_t>(readVarint());
    }

    int64_t readInt64() {
        return static_cast<int64_t>(readVarint());
    }

    int32_t readInt32() {
        return static_cast<int32_t>(readVarint());
    }

    std::vector<uint8_t> readBytes() {
        uint64_t len = readVarint();
        if (pos_ + len > size_) {
            throw std::runtime_error("Bytes length exceeds buffer");
        }
        std::vector<uint8_t> result(data_ + pos_, data_ + pos_ + len);
        pos_ += len;
        return result;
    }

    std::string readString() {
        uint64_t len = readVarint();
        if (pos_ + len > size_) {
            throw std::runtime_error("String length exceeds buffer");
        }
        std::string result(reinterpret_cast<const char*>(data_ + pos_), len);
        pos_ += len;
        return result;
    }

    void skip(WireType wireType) {
        switch (wireType) {
            case VARINT:
                readVarint();
                break;
            case I64:
                if (pos_ + 8 > size_) throw std::runtime_error("Skip I64 out of bounds");
                pos_ += 8;
                break;
            case LEN: {
                uint64_t len = readVarint();
                if (pos_ + len > size_) throw std::runtime_error("Skip LEN out of bounds");
                pos_ += len;
                break;
            }
            case I32:
                if (pos_ + 4 > size_) throw std::runtime_error("Skip I32 out of bounds");
                pos_ += 4;
                break;
            default:
                throw std::runtime_error("Unsupported wire type for skip");
        }
    }

private:
    const uint8_t* data_;
    size_t size_;
    size_t pos_;
};

static bool parseArrowPayload(ProtoReader& reader, ArrowPayload& payload) {
    while (reader.hasMore()) {
        uint32_t tag = reader.readTag();
        uint32_t field_number = tag >> 3;
        WireType wire_type = static_cast<WireType>(tag & 0x7);

        switch (field_number) {
            case 1: // schema_id (string)
                if (wire_type != LEN) return false;
                payload.schema_id = reader.readString();
                break;
            case 2: // type (enum/int32)
                if (wire_type != VARINT) return false;
                payload.type = static_cast<ArrowPayloadType>(reader.readInt32());
                break;
            case 3: // record (bytes)
                if (wire_type != LEN) return false;
                payload.record = reader.readBytes();
                break;
            default:
                // Unknown field, skip it
                reader.skip(wire_type);
                break;
        }
    }
    return true;
}

bool parseBatchArrowRecords(const uint8_t* data, size_t size, BatchArrowRecords& out) {
    try {
        ProtoReader reader(data, size);

        while (reader.hasMore()) {
            uint32_t tag = reader.readTag();
            uint32_t field_number = tag >> 3;
            WireType wire_type = static_cast<WireType>(tag & 0x7);

            switch (field_number) {
                case 1: // batch_id (int64)
                    if (wire_type != VARINT) return false;
                    out.batch_id = reader.readInt64();
                    break;
                case 2: // arrow_payloads (repeated message)
                    if (wire_type != LEN) return false;
                    {
                        std::vector<uint8_t> payload_bytes = reader.readBytes();
                        ProtoReader payload_reader(payload_bytes.data(), payload_bytes.size());
                        ArrowPayload payload;
                        if (!parseArrowPayload(payload_reader, payload)) {
                            return false;
                        }
                        out.arrow_payloads.push_back(std::move(payload));
                    }
                    break;
                case 3: // headers (bytes)
                    if (wire_type != LEN) return false;
                    out.headers = reader.readBytes();
                    break;
                default:
                    // Unknown field, skip it
                    reader.skip(wire_type);
                    break;
            }
        }
        return true;
    } catch (const std::exception& e) {
        return false;
    }
}

// Helper class for writing protobuf data
class ProtoWriter {
public:
    void writeVarint(uint64_t value) {
        while (value >= 0x80) {
            buffer_.push_back(static_cast<uint8_t>((value & 0x7F) | 0x80));
            value >>= 7;
        }
        buffer_.push_back(static_cast<uint8_t>(value & 0x7F));
    }

    void writeTag(uint32_t field_number, WireType wire_type) {
        writeVarint((field_number << 3) | wire_type);
    }

    void writeInt64(uint32_t field_number, int64_t value) {
        writeTag(field_number, VARINT);
        writeVarint(static_cast<uint64_t>(value));
    }

    void writeInt32(uint32_t field_number, int32_t value) {
        writeTag(field_number, VARINT);
        writeVarint(static_cast<uint64_t>(static_cast<uint32_t>(value)));
    }

    void writeString(uint32_t field_number, const std::string& value) {
        writeTag(field_number, LEN);
        writeVarint(value.size());
        buffer_.insert(buffer_.end(), value.begin(), value.end());
    }

    void writeBytes(uint32_t field_number, const std::vector<uint8_t>& value) {
        writeTag(field_number, LEN);
        writeVarint(value.size());
        buffer_.insert(buffer_.end(), value.begin(), value.end());
    }

    std::vector<uint8_t> finish() {
        return std::move(buffer_);
    }

private:
    std::vector<uint8_t> buffer_;
};

std::vector<uint8_t> serializeArrowPayload(const ArrowPayload& payload) {
    ProtoWriter writer;

    // Field 1: schema_id (string)
    if (!payload.schema_id.empty()) {
        writer.writeString(1, payload.schema_id);
    }

    // Field 2: type (int32 enum)
    writer.writeInt32(2, static_cast<int32_t>(payload.type));

    // Field 3: record (bytes)
    if (!payload.record.empty()) {
        writer.writeBytes(3, payload.record);
    }

    return writer.finish();
}

bool parseBatchWithSegments(
    const uint8_t* data,
    size_t size,
    BatchArrowRecords& out,
    std::vector<FieldSegment>& segments)
{
    try {
        ProtoReader reader(data, size);

        while (reader.hasMore()) {
            size_t fieldStart = reader.currentPosition();
            uint32_t tag = reader.readTag();
            uint32_t field_number = tag >> 3;
            WireType wire_type = static_cast<WireType>(tag & 0x7);

            switch (field_number) {
                case 1: // batch_id (int64)
                    if (wire_type != VARINT) return false;
                    out.batch_id = reader.readInt64();
                    {
                        size_t fieldEnd = reader.currentPosition();
                        segments.push_back({
                            .size = fieldEnd - fieldStart,
                            .fieldType = FieldSegment::BATCH_ID,
                            .payloadType = ArrowPayloadType::UNKNOWN
                        });
                    }
                    break;

                case 2: // arrow_payloads (repeated message)
                    if (wire_type != LEN) return false;
                    {
                        std::vector<uint8_t> payload_bytes = reader.readBytes();
                        ProtoReader payload_reader(payload_bytes.data(), payload_bytes.size());
                        ArrowPayload payload;
                        if (!parseArrowPayload(payload_reader, payload)) {
                            return false;
                        }

                        size_t fieldEnd = reader.currentPosition();
                        segments.push_back({
                            .size = fieldEnd - fieldStart,
                            .fieldType = FieldSegment::PAYLOAD,
                            .payloadType = payload.type
                        });

                        out.arrow_payloads.push_back(std::move(payload));
                    }
                    break;

                case 3: // headers (bytes)
                    if (wire_type != LEN) return false;
                    out.headers = reader.readBytes();
                    {
                        size_t fieldEnd = reader.currentPosition();
                        segments.push_back({
                            .size = fieldEnd - fieldStart,
                            .fieldType = FieldSegment::HEADERS,
                            .payloadType = ArrowPayloadType::UNKNOWN
                        });
                    }
                    break;

                default:
                    // Unknown field, skip it
                    reader.skip(wire_type);
                    break;
            }
        }
        return true;
    } catch (const std::exception& e) {
        return false;
    }
}

} // namespace otap
