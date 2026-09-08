// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "ProxymanClone",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(name: "ProxymanCloneKit", targets: ["ProxymanCloneKit"]),
        .executable(name: "ProxymanCloneApp", targets: ["ProxymanCloneApp"])
    ],
    targets: [
        .target(
            name: "ProxymanCloneKit",
            path: "Sources/ProxymanCloneKit"
        ),
        .executableTarget(
            name: "ProxymanCloneApp",
            dependencies: ["ProxymanCloneKit"],
            path: "Sources/ProxymanCloneApp"
        ),
        .testTarget(
            name: "ProxymanCloneKitTests",
            dependencies: ["ProxymanCloneKit"],
            path: "Tests/ProxymanCloneKitTests"
        )
    ]
)
