// swift-tools-version: 6.0
// The trainer of the AI pilots: PPO over the flight-env server, networks in MLX.

import PackageDescription

let package = Package(
    name: "flight-train",
    platforms: [.macOS("14.0")],
    products: [
        .executable(name: "flight-train", targets: ["flight-train"])
    ],
    dependencies: [
        // The only dependency: arrays, gradients and the optimiser on the GPU.
        .package(url: "https://github.com/ml-explore/mlx-swift", .upToNextMinor(from: "0.33.0"))
    ],
    targets: [
        // Everything that does not need MLX: the protocol client, the model file, running
        // statistics, GAE and a plain forward pass. Tested without a GPU.
        .target(name: "TrainerCore", path: "Sources/TrainerCore"),
        // Networks, PPO and the command line.
        .executableTarget(
            name: "flight-train",
            dependencies: [
                "TrainerCore",
                .product(name: "MLX", package: "mlx-swift"),
                .product(name: "MLXNN", package: "mlx-swift"),
                .product(name: "MLXOptimizers", package: "mlx-swift"),
            ],
            path: "Sources/flight-train"
        ),
        .testTarget(name: "TrainerCoreTests", dependencies: ["TrainerCore"], path: "Tests/TrainerCoreTests"),
    ],
    swiftLanguageModes: [.v5]
)
