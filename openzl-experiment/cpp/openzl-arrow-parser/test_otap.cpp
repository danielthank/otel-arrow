/**
 * Testing executable for OTAP OpenZL compressor
 *
 * Tests a trained OpenZL compressor on .batch files from the OTAP dataset.
 * Measures compression ratio, speed, and per-payload-type statistics.
 * Uses OpenZL C API for all compression operations.
 */

#include "otap_parser.h"
#include "otap_proto.h"
#include <openzl/openzl.h>
#include <openzl/cpp/Compressor.hpp>
#include <openzl/cpp/CCtx.hpp>
#include <openzl/cpp/DCtx.hpp>

#include <iostream>
#include <fstream>
#include <vector>
#include <string>
#include <filesystem>
#include <algorithm>
#include <chrono>
#include <unordered_map>
#include <cstring>

namespace fs = std::filesystem;

std::vector<uint8_t> readFile(const std::string& filepath) {
    std::ifstream file(filepath, std::ios::binary | std::ios::ate);
    if (!file) {
        throw std::runtime_error("Failed to open file: " + filepath);
    }

    std::streamsize size = file.tellg();
    file.seekg(0, std::ios::beg);

    std::vector<uint8_t> buffer(size);
    if (!file.read(reinterpret_cast<char*>(buffer.data()), size)) {
        throw std::runtime_error("Failed to read file: " + filepath);
    }

    return buffer;
}

std::vector<std::string> findBatchFiles(const std::string& directory) {
    std::vector<std::string> files;

    for (const auto& entry : fs::recursive_directory_iterator(directory)) {
        if (entry.is_regular_file() && entry.path().extension() == ".batch") {
            files.push_back(entry.path().string());
        }
    }

    std::sort(files.begin(), files.end());
    return files;
}

const char* payloadTypeToString(otap::ArrowPayloadType type) {
    switch (type) {
        case otap::ArrowPayloadType::UNKNOWN: return "UNKNOWN";
        case otap::ArrowPayloadType::RESOURCE_ATTRS: return "RESOURCE_ATTRS";
        case otap::ArrowPayloadType::SCOPE_ATTRS: return "SCOPE_ATTRS";
        case otap::ArrowPayloadType::UNIVARIATE_METRICS: return "UNIVARIATE_METRICS";
        case otap::ArrowPayloadType::NUMBER_DATA_POINTS: return "NUMBER_DATA_POINTS";
        case otap::ArrowPayloadType::SUMMARY_DATA_POINTS: return "SUMMARY_DATA_POINTS";
        case otap::ArrowPayloadType::HISTOGRAM_DATA_POINTS: return "HISTOGRAM_DATA_POINTS";
        case otap::ArrowPayloadType::EXP_HISTOGRAM_DATA_POINTS: return "EXP_HISTOGRAM_DATA_POINTS";
        case otap::ArrowPayloadType::NUMBER_DP_ATTRS: return "NUMBER_DP_ATTRS";
        case otap::ArrowPayloadType::SUMMARY_DP_ATTRS: return "SUMMARY_DP_ATTRS";
        case otap::ArrowPayloadType::HISTOGRAM_DP_ATTRS: return "HISTOGRAM_DP_ATTRS";
        case otap::ArrowPayloadType::EXP_HISTOGRAM_DP_ATTRS: return "EXP_HISTOGRAM_DP_ATTRS";
        case otap::ArrowPayloadType::NUMBER_DP_EXEMPLARS: return "NUMBER_DP_EXEMPLARS";
        case otap::ArrowPayloadType::HISTOGRAM_DP_EXEMPLARS: return "HISTOGRAM_DP_EXEMPLARS";
        case otap::ArrowPayloadType::EXP_HISTOGRAM_DP_EXEMPLARS: return "EXP_HISTOGRAM_DP_EXEMPLARS";
        case otap::ArrowPayloadType::NUMBER_DP_EXEMPLAR_ATTRS: return "NUMBER_DP_EXEMPLAR_ATTRS";
        case otap::ArrowPayloadType::HISTOGRAM_DP_EXEMPLAR_ATTRS: return "HISTOGRAM_DP_EXEMPLAR_ATTRS";
        case otap::ArrowPayloadType::EXP_HISTOGRAM_DP_EXEMPLAR_ATTRS: return "EXP_HISTOGRAM_DP_EXEMPLAR_ATTRS";
        case otap::ArrowPayloadType::MULTIVARIATE_METRICS: return "MULTIVARIATE_METRICS";
        case otap::ArrowPayloadType::METRIC_ATTRS: return "METRIC_ATTRS";
        case otap::ArrowPayloadType::LOGS: return "LOGS";
        case otap::ArrowPayloadType::LOG_ATTRS: return "LOG_ATTRS";
        case otap::ArrowPayloadType::SPANS: return "SPANS";
        case otap::ArrowPayloadType::SPAN_ATTRS: return "SPAN_ATTRS";
        case otap::ArrowPayloadType::SPAN_EVENTS: return "SPAN_EVENTS";
        case otap::ArrowPayloadType::SPAN_LINKS: return "SPAN_LINKS";
        case otap::ArrowPayloadType::SPAN_EVENT_ATTRS: return "SPAN_EVENT_ATTRS";
        case otap::ArrowPayloadType::SPAN_LINK_ATTRS: return "SPAN_LINK_ATTRS";
        default: return "UNKNOWN";
    }
}

struct PayloadStats {
    size_t count = 0;
    size_t totalOriginalSize = 0;
};

int main(int argc, char** argv) {
    if (argc != 3) {
        std::cerr << "Usage: " << argv[0] << " <input_directory> <compressor_file>" << std::endl;
        std::cerr << "  input_directory: Directory containing .batch files (e.g., new/)" << std::endl;
        std::cerr << "  compressor_file: Path to trained compressor file (e.g., trained.zl)" << std::endl;
        return 1;
    }

    const std::string inputDir = argv[1];
    const std::string compressorPath = argv[2];

    try {
        std::cout << "==================================================\n";
        std::cout << "OTAP OpenZL Compressor Testing\n";
        std::cout << "==================================================\n";
        std::cout << "Input directory: " << inputDir << std::endl;
        std::cout << "Compressor file: " << compressorPath << std::endl;
        std::cout << std::endl;

        // Load compressor
        std::cout << "Loading compressor..." << std::endl;
        auto compressorData = readFile(compressorPath);

        openzl::Compressor compressor;

        // Register dependencies first
        otap::registerOtapCompressionGraph(compressor);

        // Deserialize
        compressor.deserialize(openzl::poly::string_view(
            (const char*)compressorData.data(), compressorData.size()));

        std::cout << "Compressor loaded successfully" << std::endl;

        // Find all .batch files
        std::cout << "\nSearching for .batch files..." << std::endl;
        auto batchFiles = findBatchFiles(inputDir);
        std::cout << "Found " << batchFiles.size() << " .batch files" << std::endl;

        if (batchFiles.empty()) {
            std::cerr << "Error: No .batch files found in " << inputDir << std::endl;
            return 1;
        }

        // Statistics
        size_t totalCompressedSize = 0;
        size_t totalUncompressedSize = 0;
        size_t totalCompressTimeUs = 0;
        size_t totalDecompressTimeUs = 0;
        size_t filesCompressed = 0;
        std::unordered_map<int32_t, PayloadStats> payloadStats;

        std::cout << "\nCompressing test files..." << std::endl;

        // Compress each file
        for (const auto& filepath : batchFiles) {
            auto data = readFile(filepath);

            // Parse to collect payload type stats
            otap::BatchArrowRecords batch;
            if (otap::parseBatchArrowRecords(data.data(), data.size(), batch)) {
                for (const auto& payload : batch.arrow_payloads) {
                    int32_t type_id = static_cast<int32_t>(payload.type);
                    payloadStats[type_id].count++;
                    payloadStats[type_id].totalOriginalSize += payload.record.size();
                }
            }

            // Create fresh contexts for each file
            openzl::CCtx cctx;
            openzl::DCtx dctx;
            cctx.refCompressor(compressor);
            cctx.setParameter(openzl::CParam::FormatVersion, ZL_MAX_FORMAT_VERSION);

            // Compress using C++ wrapper
            std::string compressed;
            compressed.resize(openzl::compressBound(data.size()));

            auto start = std::chrono::high_resolution_clock::now();
            size_t csize = cctx.compressSerial(compressed,
                openzl::poly::string_view((const char*)data.data(), data.size()));
            auto end = std::chrono::high_resolution_clock::now();
            compressed.resize(csize);

            auto duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);
            totalCompressTimeUs += duration.count();

            // Decompress to verify
            start = std::chrono::high_resolution_clock::now();
            std::string decompressed = dctx.decompressSerial(
                openzl::poly::string_view(compressed.data(), compressed.size()));
            end = std::chrono::high_resolution_clock::now();
            duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);
            totalDecompressTimeUs += duration.count();

            // Verify round-trip
            if (decompressed.size() != data.size() ||
                std::memcmp(decompressed.data(), data.data(), data.size()) != 0) {
                std::cerr << "Warning: Round-trip verification failed for " << filepath << std::endl;
                continue;
            }

            totalCompressedSize += csize;
            totalUncompressedSize += data.size();
            filesCompressed++;
        }

        // Print results
        std::cout << "\n==================================================\n";
        std::cout << "Compression Results\n";
        std::cout << "==================================================\n";
        std::cout << "Files compressed: " << filesCompressed << " / " << batchFiles.size() << std::endl;
        std::cout << "Original size: " << (totalUncompressedSize / 1024.0 / 1024.0) << " MB" << std::endl;
        std::cout << "Compressed size: " << (totalCompressedSize / 1024.0 / 1024.0) << " MB" << std::endl;
        std::cout << "Compression ratio: " << (totalUncompressedSize / (double)totalCompressedSize) << "x" << std::endl;
        std::cout << "Compression speed: " << (totalUncompressedSize / (double)totalCompressTimeUs) << " MB/s" << std::endl;
        std::cout << "Decompression speed: " << (totalUncompressedSize / (double)totalDecompressTimeUs) << " MB/s" << std::endl;

        std::cout << "\n==================================================\n";
        std::cout << "Per-Payload-Type Statistics\n";
        std::cout << "==================================================\n";
        std::cout << "Type                                 | Count  | Size (MB)" << std::endl;
        std::cout << "-------------------------------------|--------|------------" << std::endl;

        for (const auto& [type_id, stats] : payloadStats) {
            otap::ArrowPayloadType type = static_cast<otap::ArrowPayloadType>(type_id);
            printf("%-36s | %6zu | %10.2f\n",
                   payloadTypeToString(type),
                   stats.count,
                   stats.totalOriginalSize / 1024.0 / 1024.0);
        }

        std::cout << "\n==================================================\n";
        std::cout << "Testing completed!\n";
        std::cout << "==================================================\n";

        return 0;

    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
}
