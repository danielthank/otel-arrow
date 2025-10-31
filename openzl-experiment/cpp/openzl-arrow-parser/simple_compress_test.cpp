/**
 * Simple OpenZL Compression Test
 *
 * Tests generic OpenZL compression on Arrow files to establish baseline
 * Uses OpenZL C API
 */

#include <openzl/openzl.h>

#include <iostream>
#include <fstream>
#include <vector>
#include <string>
#include <cstring>
#include <filesystem>
#include <chrono>

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

int main(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "Usage: " << argv[0] << " <arrow_file>" << std::endl;
        return 1;
    }

    std::string filepath = argv[1];

    try {
        // Read file
        std::cout << "Reading file: " << filepath << std::endl;
        auto data = readFile(filepath);
        std::cout << "File size: " << data.size() << " bytes" << std::endl;

        // Compress with generic compression using C API
        auto start = std::chrono::high_resolution_clock::now();

        size_t compressed_size_alloc = ZL_compressBound(data.size());
        std::vector<uint8_t> compressed(compressed_size_alloc);

        ZL_CCtx* cctx = ZL_CCtx_create();
        if (!cctx) {
            throw std::runtime_error("Failed to create compression context");
        }

        // Set format version (required parameter)
        ZL_Report setparam_report = ZL_CCtx_setParameter(cctx, ZL_CParam_formatVersion, ZL_getDefaultEncodingVersion());
        if (ZL_isError(setparam_report)) {
            std::string error_msg = ZL_CCtx_getErrorContextString(cctx, setparam_report);
            ZL_CCtx_free(cctx);
            throw std::runtime_error("Failed to set format version: " + error_msg);
        }

        ZL_Report compress_report = ZL_CCtx_compress(
            cctx,
            compressed.data(), compressed.size(),
            data.data(), data.size()
        );

        if (ZL_isError(compress_report)) {
            std::string error_msg = ZL_CCtx_getErrorContextString(cctx, compress_report);
            ZL_CCtx_free(cctx);
            throw std::runtime_error("Compression failed: " + error_msg);
        }

        size_t compressed_size = ZL_validResult(compress_report);
        ZL_CCtx_free(cctx);
        compressed.resize(compressed_size);

        auto end = std::chrono::high_resolution_clock::now();
        auto duration = std::chrono::duration_cast<std::chrono::microseconds>(end - start);

        // Print results
        std::cout << "\nOpenZL Generic Compression Results:" << std::endl;
        std::cout << "  Original size: " << data.size() << " bytes" << std::endl;
        std::cout << "  Compressed size: " << compressed.size() << " bytes" << std::endl;
        std::cout << "  Compression ratio: " << (data.size() / (double)compressed.size()) << "x" << std::endl;
        std::cout << "  Time: " << (duration.count() / 1000.0) << " ms" << std::endl;

        // Decompress to verify using C API
        std::vector<uint8_t> decompressed(data.size());
        ZL_DCtx* dctx = ZL_DCtx_create();
        if (!dctx) {
            throw std::runtime_error("Failed to create decompression context");
        }

        ZL_Report decompress_report = ZL_DCtx_decompress(
            dctx,
            decompressed.data(), decompressed.size(),
            compressed.data(), compressed.size()
        );

        if (ZL_isError(decompress_report)) {
            std::string error_msg = ZL_DCtx_getErrorContextString(dctx, decompress_report);
            ZL_DCtx_free(dctx);
            throw std::runtime_error("Decompression failed: " + error_msg);
        }

        size_t decompressed_size = ZL_validResult(decompress_report);
        ZL_DCtx_free(dctx);
        decompressed.resize(decompressed_size);

        if (decompressed.size() == data.size() &&
            std::memcmp(decompressed.data(), data.data(), data.size()) == 0) {
            std::cout << "  Verification: PASSED" << std::endl;
        } else {
            std::cerr << "  Verification: FAILED!" << std::endl;
            std::cerr << "    Expected size: " << data.size() << std::endl;
            std::cerr << "    Got size: " << decompressed.size() << std::endl;
            return 1;
        }

        return 0;

    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
}