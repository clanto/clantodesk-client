import UIKit
import Flutter

@main
@objc class AppDelegate: FlutterAppDelegate {
  private var clantoFileExporter: ClantoFileExporter?
  private var clantoManagedJson = ""

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    GeneratedPluginRegistrant.register(with: self)
    if let controller = window?.rootViewController as? FlutterViewController {
      let exporter = ClantoFileExporter(presenter: controller)
      clantoFileExporter = exporter
      let channel = FlutterMethodChannel(
        name: "mChannel",
        binaryMessenger: controller.binaryMessenger
      )
      channel.setMethodCallHandler { [weak exporter] call, result in
        if exporter?.handle(call, result: result) != true {
          result(FlutterMethodNotImplemented)
        }
      }
      // Clanto: fotocamera come webcam del tecnico (forwarding.dart).
      ClantoCamera.register(messenger: controller.binaryMessenger)
    }
    // Clanto: configurazione gestita da MDM, all'avvio e a ogni modifica.
    applyClantoManagedConfig()
    NotificationCenter.default.addObserver(
      forName: UserDefaults.didChangeNotification, object: nil, queue: .main
    ) { [weak self] _ in self?.applyClantoManagedConfig() }
    dummyMethodToEnforceBundling();
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }
    
  /// Managed app configuration (src/clanto/managed_config.rs): solo testi,
  /// numeri e booleani, inviata a Rust solo quando cambia.
  private func applyClantoManagedConfig() {
    let managed = UserDefaults.standard.dictionary(forKey: "com.apple.configuration.managed") ?? [:]
    let values = managed.filter { $0.value is String || $0.value is NSNumber }
    guard let data = try? JSONSerialization.data(withJSONObject: values, options: [.sortedKeys]),
          let json = String(data: data, encoding: .utf8),
          json != clantoManagedJson
    else { return }
    clantoManagedJson = json
    json.withCString { clanto_apply_managed_config($0) }
  }

  public func dummyMethodToEnforceBundling() {
      dummy_method_to_enforce_bundling();
    session_get_rgba(nil, 0);
  }
}
