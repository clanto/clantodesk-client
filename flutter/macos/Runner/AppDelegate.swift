import Cocoa
import FlutterMacOS
import SystemExtensions

@main
class AppDelegate: FlutterAppDelegate {
    var launched = false;
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
      dummy_method_to_enforce_bundling()
    // https://github.com/leanflutter/window_manager/issues/214
    return false
  }

    override func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool {
        if (launched) {
            handle_applicationShouldOpenUntitledFile();
        }
        return true
    }

    override func applicationDidFinishLaunching(_ aNotification: Notification) {
        launched = true;
        NSApplication.shared.activate(ignoringOtherApps: true);
        // Clanto: attivazione della Camera Extension (flutter/lib/clanto/forwarding.dart).
        if #available(macOS 10.15, *), let controller = mainFlutterWindow?.contentViewController as? FlutterViewController {
            ClantoCameraExtension.register(messenger: controller.engine.binaryMessenger)
        }
    }
}

/// Chiede a macOS di attivare la Camera Extension incorporata nell'app.
/// Risponde "activated", "approval" (da approvare in Impostazioni di Sistema) o un errore.
@available(macOS 10.15, *)
class ClantoCameraExtension: NSObject, OSSystemExtensionRequestDelegate {
    static let identifier = "it.clanto.clantodesk.camera"
    static var shared: ClantoCameraExtension?
    private var result: FlutterResult?

    static func register(messenger: FlutterBinaryMessenger) {
        let channel = FlutterMethodChannel(name: "it.clanto.clantodesk/camera_extension", binaryMessenger: messenger)
        channel.setMethodCallHandler { call, result in
            guard call.method == "activate" else {
                result(FlutterMethodNotImplemented)
                return
            }
            guard #available(macOS 13.0, *) else {
                result(FlutterError(code: "unsupported", message: "The virtual camera requires macOS 13", details: nil))
                return
            }
            let delegate = ClantoCameraExtension()
            delegate.result = result
            shared = delegate
            let request = OSSystemExtensionRequest.activationRequest(forExtensionWithIdentifier: identifier, queue: .main)
            request.delegate = delegate
            OSSystemExtensionManager.shared.submitRequest(request)
        }
    }

    private func finish(_ value: Any?) {
        result?(value)
        result = nil
        ClantoCameraExtension.shared = nil
    }

    func request(_ request: OSSystemExtensionRequest, actionForReplacingExtension existing: OSSystemExtensionProperties, withExtension ext: OSSystemExtensionProperties) -> OSSystemExtensionRequest.ReplacementAction {
        return .replace
    }

    func requestNeedsUserApproval(_ request: OSSystemExtensionRequest) {
        // La richiesta resta aperta finché l'utente non approva: si risponde subito.
        result?("approval")
        result = nil
    }

    func request(_ request: OSSystemExtensionRequest, didFinishWithResult result: OSSystemExtensionRequest.Result) {
        finish("activated")
    }

    func request(_ request: OSSystemExtensionRequest, didFailWithError error: Error) {
        finish(FlutterError(code: "failed", message: error.localizedDescription, details: nil))
    }
}
