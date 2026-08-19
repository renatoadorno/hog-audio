// swift-tools-version:6.0
import PackageDescription

// Caminho relativo ao diretório do pacote: verificado que o SPM resolve corretamente.
let rustLib = "../../rust/target/release"

let linkRust: [LinkerSetting] = [
    .unsafeFlags(["-L\(rustLib)", "-lhog_audio"]),
    .linkedFramework("CoreAudio"),
    .linkedFramework("AudioToolbox"),
    .linkedFramework("CoreFoundation"),
]

let package = Package(
    name: "HogPlayer",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "HogAudioFFI"),
        .target(name: "HogAudioBindings", dependencies: ["HogAudioFFI"]),
        .target(name: "HogPlayerKit", dependencies: ["HogAudioBindings"]),
        .executableTarget(
            name: "HogPlayer",
            dependencies: ["HogPlayerKit"],
            linkerSettings: linkRust
        ),
        .testTarget(
            name: "HogPlayerKitTests",
            dependencies: ["HogPlayerKit"],
            linkerSettings: linkRust
        ),
    ]
)
