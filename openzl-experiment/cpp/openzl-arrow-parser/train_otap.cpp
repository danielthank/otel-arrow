/**
 * Training executable for OTAP OpenZL compressor with ACE support
 *
 * Trains an OpenZL compressor on .batch files from the OTAP dataset using
 * OpenZL's ACE (Automated Compressor Explorer) training infrastructure.
 */

#include "otap_parser.h"
#include "otap_proto.h"
#include <openzl/openzl.h>
#include <openzl/cpp/Compressor.hpp>
#include <openzl/cpp/CCtx.hpp>

// OpenZL training API
#include "tools/training/train.h"
#include "tools/training/utils/utils.h"
#include "tools/io/InputSetBuilder.h"
#include "tools/io/InputSetStatic.h"

#include <iostream>
#include <fstream>
#include <vector>
#include <string>
#include <filesystem>
#include <algorithm>
#include <random>
#include <cstring>

namespace fs = std::filesystem;

/**
 * Compressor recreation callback for ACE training
 *
 * During iterative training, OpenZL needs to recreate compressor instances
 * from serialized state. This callback MUST register all custom graphs
 * (OTAP parser + clustering) before deserializing.
 */
static std::unique_ptr<openzl::Compressor> createCompressorFromSerialized(
        openzl::poly::string_view serialized)
{
    auto compressor = std::make_unique<openzl::Compressor>();

    // CRITICAL: Register OTAP graphs before deserializing
    otap::registerOtapCompressionGraph(*compressor);

    // Deserialize the trained state
    compressor->deserialize(serialized);

    return compressor;
}

int main(int argc, char** argv) {
    // Parse command-line arguments
    if (argc < 3 || argc > 6) {
        std::cerr << "Usage: " << argv[0] << " <input_directory> <output_compressor_file> [OPTIONS]" << std::endl;
        std::cerr << "  input_directory: Directory containing .batch files (e.g., new/)" << std::endl;
        std::cerr << "  output_compressor_file: Path to save compressor (e.g., trained.zl)" << std::endl;
        std::cerr << "\nOptions:" << std::endl;
        std::cerr << "  --sample-rate RATE: Fraction of files to randomly sample (0.0-1.0)" << std::endl;
        std::cerr << "                      Examples: 0.1 = 10%, 0.5 = 50%, 1.0 = 100% (default)" << std::endl;
        std::cerr << "  --skip-training:    Save untrained parser compressor (no ACE training)" << std::endl;
        std::cerr << "                      When set, input_directory is ignored" << std::endl;
        return 1;
    }

    const std::string inputDir = argv[1];
    const std::string outputPath = argv[2];

    // Parse optional flags
    double sampleRate = 1.0;  // Default: use all files
    bool skipTraining = false;

    for (int i = 3; i < argc; ++i) {
        if (std::strcmp(argv[i], "--sample-rate") == 0) {
            if (i + 1 >= argc) {
                std::cerr << "Error: --sample-rate requires a value (0.0-1.0)" << std::endl;
                return 1;
            }
            sampleRate = std::atof(argv[i + 1]);
            if (sampleRate <= 0.0 || sampleRate > 1.0) {
                std::cerr << "Error: sample rate must be in range (0.0, 1.0], got " << sampleRate << std::endl;
                return 1;
            }
            ++i;  // Skip next argument (the rate value)
        } else if (std::strcmp(argv[i], "--skip-training") == 0) {
            skipTraining = true;
        } else {
            std::cerr << "Error: unknown option '" << argv[i] << "'" << std::endl;
            std::cerr << "Valid options: --sample-rate RATE, --skip-training" << std::endl;
            return 1;
        }
    }

    try {
        std::cout << "==================================================\n";
        if (skipTraining) {
            std::cout << "OTAP OpenZL Compressor Generator (Untrained)\n";
        } else {
            std::cout << "OTAP OpenZL Compressor Training (ACE-enabled)\n";
        }
        std::cout << "==================================================\n";
        if (!skipTraining) {
            std::cout << "Input directory: " << inputDir << std::endl;
        }
        std::cout << "Output file: " << outputPath << std::endl;
        if (sampleRate < 1.0 && !skipTraining) {
            std::cout << "Sample rate: " << (sampleRate * 100.0) << "%" << std::endl;
        }
        std::cout << std::endl;

        // Create compressor and register OTAP graphs
        std::cout << "Registering OTAP compression graphs...\n";
        openzl::Compressor compressor;
        ZL_GraphID graphId = otap::registerOtapCompressionGraph(compressor);
        compressor.selectStartingGraph(graphId);
        std::cout << "Graphs registered successfully\n\n";

        std::string serialized;

        if (skipTraining) {
            // Skip training - serialize immediately with default configuration
            std::cout << "Skipping training - generating untrained parser compressor\n";
            serialized = compressor.serialize();
            std::cout << "Untrained compressor size: " << serialized.size() << " bytes\n\n";
        } else {
            // Load input dataset using InputSetBuilder
            std::cout << "Loading .batch files from: " << inputDir << std::endl;
            auto allInputs = openzl::tools::io::InputSetBuilder(true)  // recursive
                .add_path(inputDir)
                .build();

            // Collect all inputs into a vector for sampling
            std::vector<std::shared_ptr<openzl::tools::io::Input>> inputVec;
            size_t totalFileCount = 0;
            size_t totalBytesAll = 0;
            for (const auto& input : *allInputs) {
                inputVec.push_back(input);
                totalFileCount++;
                totalBytesAll += input->contents().size();
            }

            if (totalFileCount == 0) {
                std::cerr << "Error: No .batch files found in " << inputDir << std::endl;
                return 1;
            }

            std::cout << "Found " << totalFileCount << " files, "
                      << (totalBytesAll / 1024.0 / 1024.0) << " MB total\n";

            // Apply random sampling if sample rate < 1.0
            std::vector<std::shared_ptr<openzl::tools::io::Input>> sampledInputs;
            size_t selectedFileCount = 0;
            size_t selectedBytes = 0;

            if (sampleRate < 1.0) {
                // Calculate target number of files
                size_t targetCount = static_cast<size_t>(std::ceil(totalFileCount * sampleRate));
                if (targetCount == 0) targetCount = 1;  // Always sample at least 1 file

                std::cout << "Randomly sampling " << (sampleRate * 100.0) << "% of files...\n";

                // Random shuffle with fixed seed for reproducibility
                std::mt19937 rng(42);  // Fixed seed for deterministic behavior
                std::shuffle(inputVec.begin(), inputVec.end(), rng);

                // Select first N files after shuffle
                for (size_t i = 0; i < targetCount && i < inputVec.size(); ++i) {
                    sampledInputs.push_back(inputVec[i]);
                    selectedFileCount++;
                    selectedBytes += inputVec[i]->contents().size();
                }

                std::cout << "Selected " << selectedFileCount << " of " << totalFileCount
                          << " files (" << (100.0 * selectedFileCount / totalFileCount) << "%) = "
                          << (selectedBytes / 1024.0 / 1024.0) << " MB of "
                          << (totalBytesAll / 1024.0 / 1024.0) << " MB\n\n";
            } else {
                // Use all files
                sampledInputs = std::move(inputVec);
                selectedFileCount = totalFileCount;
                selectedBytes = totalBytesAll;
                std::cout << "Using all " << selectedFileCount << " files\n\n";
            }

            // Convert sampled inputs back to InputSet using InputSetStatic
            auto inputs = std::make_unique<openzl::tools::io::InputSetStatic>(sampledInputs);

            // Configure ACE training parameters
            std::cout << "Configuring ACE training...\n";
            openzl::training::TrainParams trainParams = {
                .compressorGenFunc = createCompressorFromSerialized,
                .threads = 16,
                .clusteringTrainer = openzl::training::ClusteringTrainer::Greedy,
            };
            std::cout << "  Trainer: Greedy\n";
            std::cout << "  Threads: 16\n\n";

            // Run ACE training
            std::cout << "Starting ACE training...\n";
            std::cout << "(This may take several minutes depending on dataset size)\n\n";

            auto multiInputs = openzl::training::inputSetToMultiInputs(*inputs);
            auto serializedList = openzl::training::train(multiInputs, compressor, trainParams);

            if (serializedList.empty()) {
                throw std::runtime_error("Training failed: no compressor returned");
            }

            auto& serializedPtr = serializedList[0];
            serialized = std::string(serializedPtr->data(), serializedPtr->size());
            std::cout << "\nTraining completed!\n";
            std::cout << "Learned compressor size: " << serialized.size() << " bytes\n\n";
        }

        // Save serialized compressor
        std::cout << "Saving compressor to: " << outputPath << "\n";
        std::ofstream output(outputPath, std::ios::binary);
        if (!output) {
            throw std::runtime_error("Failed to open output file");
        }
        output.write(serialized.data(), serialized.size());
        output.close();
        std::cout << "Compressor saved successfully\n\n";

        std::cout << "==================================================\n";
        if (skipTraining) {
            std::cout << "Untrained Compressor Generated!\n";
        } else {
            std::cout << "ACE Training Complete!\n";
        }
        std::cout << "==================================================\n";
        if (!skipTraining) {
            std::cout << "\nTrained compressor ready for evaluation.\n";
            std::cout << "Run: ./test_otap " << inputDir << " " << outputPath << std::endl;
        } else {
            std::cout << "\nUntrained parser compressor ready for use.\n";
            std::cout << "This compressor has OTAP parsing enabled but no learned optimizations.\n";
        }
        std::cout << std::endl;

        return 0;

    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
}
