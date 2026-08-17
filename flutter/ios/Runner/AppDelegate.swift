import UIKit
import Flutter

@main
@objc class AppDelegate: FlutterAppDelegate {
  private var clantoFileExporter: ClantoFileExporter?

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
    }
    dummyMethodToEnforceBundling();
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }
    
  public func dummyMethodToEnforceBundling() {
      dummy_method_to_enforce_bundling();
    session_get_rgba(nil, 0);
  }
}
