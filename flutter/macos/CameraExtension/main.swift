// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Camera Extension di ClantoDesk (macOS 13+): una webcam "ClantoDesk Camera" con
// due stream. Nello stream sink l'host ClantoDesk scrive la webcam del tecnico
// (libs/clanto-vcam/src/mac.rs); lo stream di uscita la consegna alle
// applicazioni a 30 fps. Senza frame recenti, immagine nera.
// Costruita e firmata da .github/scripts/build-macos-camera-extension.sh.

import CoreMediaIO
import CoreVideo
import Foundation
import IOKit.audio
import os.log

let kName = "ClantoDesk Camera"
let kWidth: Int32 = 1280
let kHeight: Int32 = 720
let kFrameRate: Int32 = 30
/// Oltre questo intervallo senza frame dall'host si torna al nero.
let kStaleAfter: TimeInterval = 1.0
let logger = OSLog(subsystem: "it.clanto.clantodesk.camera", category: "extension")

func hostTimeNanoseconds() -> UInt64 {
    let t = CMClockGetTime(CMClockGetHostTimeClock())
    return UInt64(max(0, t.seconds) * Double(NSEC_PER_SEC))
}

class StreamSource: NSObject, CMIOExtensionStreamSource {
    private(set) var stream: CMIOExtensionStream!
    let device: CMIOExtensionDevice
    let format: CMIOExtensionStreamFormat
    let isSink: Bool
    var client: CMIOExtensionClient?

    init(localizedName: String, streamID: UUID, format: CMIOExtensionStreamFormat, device: CMIOExtensionDevice, sink: Bool) {
        self.device = device
        self.format = format
        self.isSink = sink
        super.init()
        stream = CMIOExtensionStream(
            localizedName: localizedName,
            streamID: streamID,
            direction: sink ? .sink : .source,
            clockType: .hostTime,
            source: self)
    }

    var formats: [CMIOExtensionStreamFormat] { [format] }

    var availableProperties: Set<CMIOExtensionProperty> {
        var props: Set<CMIOExtensionProperty> = [.streamActiveFormatIndex, .streamFrameDuration]
        if isSink {
            props.formUnion([.streamSinkBufferQueueSize, .streamSinkBuffersRequiredForStartup])
        }
        return props
    }

    func streamProperties(forProperties properties: Set<CMIOExtensionProperty>) throws -> CMIOExtensionStreamProperties {
        let p = CMIOExtensionStreamProperties(dictionary: [:])
        if properties.contains(.streamActiveFormatIndex) {
            p.activeFormatIndex = 0
        }
        if properties.contains(.streamFrameDuration) {
            p.frameDuration = CMTime(value: 1, timescale: kFrameRate)
        }
        if isSink {
            if properties.contains(.streamSinkBufferQueueSize) {
                p.sinkBufferQueueSize = 2
            }
            if properties.contains(.streamSinkBuffersRequiredForStartup) {
                p.sinkBuffersRequiredForStartup = 1
            }
        }
        return p
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {}

    func authorizedToStartStream(for client: CMIOExtensionClient) -> Bool {
        if isSink {
            self.client = client
        }
        return true
    }

    func startStream() throws {
        guard let source = device.source as? DeviceSource else { return }
        if isSink {
            if let client = client {
                source.startSinking(stream: stream, client: client)
            }
        } else {
            source.startStreaming()
        }
    }

    func stopStream() throws {
        guard let source = device.source as? DeviceSource else { return }
        if isSink {
            source.stopSinking()
        } else {
            source.stopStreaming()
        }
    }
}

class DeviceSource: NSObject, CMIOExtensionDeviceSource {
    private(set) var device: CMIOExtensionDevice!
    private var output: StreamSource!
    private var sink: StreamSource!
    private var pool: CVPixelBufferPool?
    private var formatDescription: CMFormatDescription?
    private let queue = DispatchQueue(label: "it.clanto.clantodesk.camera.frames", qos: .userInteractive)
    private var timer: DispatchSourceTimer?
    private var streaming = 0
    private var sinking = false
    private var lastImage: CVPixelBuffer?
    private var lastImageAt = Date.distantPast

    init(localizedName: String) {
        super.init()
        device = CMIOExtensionDevice(
            localizedName: localizedName,
            deviceID: UUID(uuidString: "6B1E4C2A-9F3D-4A57-8E21-C0D4F7A9B135")!,
            legacyDeviceID: nil,
            source: self)
        var description: CMFormatDescription?
        CMVideoFormatDescriptionCreate(
            allocator: kCFAllocatorDefault,
            codecType: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            width: kWidth, height: kHeight, extensions: nil,
            formatDescriptionOut: &description)
        formatDescription = description
        let attributes: NSDictionary = [
            kCVPixelBufferWidthKey: kWidth,
            kCVPixelBufferHeightKey: kHeight,
            kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferIOSurfacePropertiesKey: [:] as NSDictionary,
        ]
        CVPixelBufferPoolCreate(kCFAllocatorDefault, nil, attributes, &pool)
        let frameDuration = CMTime(value: 1, timescale: kFrameRate)
        let format = CMIOExtensionStreamFormat(
            formatDescription: description!,
            maxFrameDuration: frameDuration,
            minFrameDuration: frameDuration,
            validFrameDurations: nil)
        output = StreamSource(
            localizedName: "\(localizedName).Video",
            streamID: UUID(uuidString: "2D7A9E41-5B3C-4F86-A1D0-93E6C8B47F12")!,
            format: format, device: device, sink: false)
        sink = StreamSource(
            localizedName: "\(localizedName).Sink",
            streamID: UUID(uuidString: "8C4F1B37-E2A6-4D95-B7F3-1A6E0D29C584")!,
            format: format, device: device, sink: true)
        do {
            try device.addStream(output.stream)
            try device.addStream(sink.stream)
        } catch {
            os_log(.error, log: logger, "addStream: %{public}@", error.localizedDescription)
        }
    }

    var availableProperties: Set<CMIOExtensionProperty> { [.deviceTransportType, .deviceModel] }

    func deviceProperties(forProperties properties: Set<CMIOExtensionProperty>) throws -> CMIOExtensionDeviceProperties {
        let p = CMIOExtensionDeviceProperties(dictionary: [:])
        if properties.contains(.deviceTransportType) {
            p.transportType = kIOAudioDeviceTransportTypeVirtual
        }
        if properties.contains(.deviceModel) {
            p.model = kName
        }
        return p
    }

    func setDeviceProperties(_ deviceProperties: CMIOExtensionDeviceProperties) throws {}

    func startStreaming() {
        queue.async {
            self.streaming += 1
            guard self.timer == nil else { return }
            let timer = DispatchSource.makeTimerSource(flags: .strict, queue: self.queue)
            timer.schedule(deadline: .now(), repeating: 1.0 / Double(kFrameRate), leeway: .milliseconds(2))
            timer.setEventHandler { [weak self] in self?.sendFrame() }
            timer.resume()
            self.timer = timer
        }
    }

    func stopStreaming() {
        queue.async {
            self.streaming = max(0, self.streaming - 1)
            if self.streaming == 0 {
                self.timer?.cancel()
                self.timer = nil
            }
        }
    }

    func startSinking(stream: CMIOExtensionStream, client: CMIOExtensionClient) {
        queue.async {
            self.sinking = true
            self.consume(stream: stream, client: client)
        }
    }

    func stopSinking() {
        queue.async {
            self.sinking = false
            self.lastImage = nil
        }
    }

    private func consume(stream: CMIOExtensionStream, client: CMIOExtensionClient) {
        guard sinking else { return }
        stream.consumeSampleBuffer(from: client) { [weak self] buffer, sequence, _, hasMore, _ in
            guard let self = self else { return }
            self.queue.async {
                if let buffer = buffer, let image = CMSampleBufferGetImageBuffer(buffer) {
                    self.lastImage = image
                    self.lastImageAt = Date()
                    stream.notifyScheduledOutputChanged(
                        CMIOExtensionScheduledOutput(sequenceNumber: sequence, hostTimeInNanoseconds: hostTimeNanoseconds()))
                }
                let delay: DispatchTimeInterval = hasMore ? .milliseconds(0) : .milliseconds(5)
                self.queue.asyncAfter(deadline: .now() + delay) {
                    self.consume(stream: stream, client: client)
                }
            }
        }
    }

    private func blackImage() -> CVPixelBuffer? {
        guard let pool = pool else { return nil }
        var buffer: CVPixelBuffer?
        guard CVPixelBufferPoolCreatePixelBuffer(kCFAllocatorDefault, pool, &buffer) == kCVReturnSuccess,
              let image = buffer else { return nil }
        CVPixelBufferLockBaseAddress(image, [])
        for (plane, value) in [(0, UInt8(16)), (1, UInt8(128))] {
            if let base = CVPixelBufferGetBaseAddressOfPlane(image, plane) {
                memset(base, Int32(value), CVPixelBufferGetBytesPerRowOfPlane(image, plane) * CVPixelBufferGetHeightOfPlane(image, plane))
            }
        }
        CVPixelBufferUnlockBaseAddress(image, [])
        return image
    }

    private func sendFrame() {
        let fresh = lastImage != nil && Date().timeIntervalSince(lastImageAt) < kStaleAfter
        guard let image = fresh ? lastImage : blackImage() else { return }
        var description: CMFormatDescription?
        guard CMVideoFormatDescriptionCreateForImageBuffer(
            allocator: kCFAllocatorDefault, imageBuffer: image, formatDescriptionOut: &description) == noErr,
              let format = description else { return }
        let now = hostTimeNanoseconds()
        var timing = CMSampleTimingInfo(
            duration: CMTime(value: 1, timescale: kFrameRate),
            presentationTimeStamp: CMTime(value: CMTimeValue(now), timescale: CMTimeScale(NSEC_PER_SEC)),
            decodeTimeStamp: .invalid)
        var sample: CMSampleBuffer?
        guard CMSampleBufferCreateForImageBuffer(
            allocator: kCFAllocatorDefault, imageBuffer: image, dataReady: true,
            makeDataReadyCallback: nil, refcon: nil, formatDescription: format,
            sampleTiming: &timing, sampleBufferOut: &sample) == noErr,
              let buffer = sample else { return }
        output.stream.send(buffer, discontinuity: [], hostTimeInNanoseconds: now)
    }
}

class ProviderSource: NSObject, CMIOExtensionProviderSource {
    private(set) var provider: CMIOExtensionProvider!
    private var deviceSource: DeviceSource!

    init(clientQueue: DispatchQueue?) {
        super.init()
        provider = CMIOExtensionProvider(source: self, clientQueue: clientQueue)
        deviceSource = DeviceSource(localizedName: kName)
        do {
            try provider.addDevice(deviceSource.device)
        } catch {
            os_log(.error, log: logger, "addDevice: %{public}@", error.localizedDescription)
        }
    }

    func connect(to client: CMIOExtensionClient) throws {}

    func disconnect(from client: CMIOExtensionClient) {}

    var availableProperties: Set<CMIOExtensionProperty> { [.providerManufacturer] }

    func providerProperties(forProperties properties: Set<CMIOExtensionProperty>) throws -> CMIOExtensionProviderProperties {
        let p = CMIOExtensionProviderProperties(dictionary: [:])
        if properties.contains(.providerManufacturer) {
            p.manufacturer = "Clanto Services"
        }
        return p
    }

    func setProviderProperties(_ providerProperties: CMIOExtensionProviderProperties) throws {}
}

let providerSource = ProviderSource(clientQueue: nil)
CMIOExtensionProvider.startService(provider: providerSource.provider)
CFRunLoopRun()
