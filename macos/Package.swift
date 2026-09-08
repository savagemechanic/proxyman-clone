// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "ProxymanClone",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "ProxymanCloneApp", targets: ["ProxymanCloneApp"])
    ],
    targets: [
        .executableTarget(
            name: "ProxymanCloneApp",
            path: "Sources/ProxymanCloneApp"
        ),
        .testTarget(
            name: "ProxymanCloneAppTests",
            dependencies: ["ProxymanCloneApp"],
            path: "Tests/ProxymanCloneAppTests"
        )
    ]
)
