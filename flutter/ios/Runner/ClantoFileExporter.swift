// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import Flutter
import UIKit

/// Esporta nell'app File i documenti ricevuti da un computer remoto.
/// Le richieste vengono accodate per evitare di presentare più picker insieme.
final class ClantoFileExporter: NSObject, UIDocumentPickerDelegate {
  private struct ExportRequest {
    let sourceURL: URL
    let result: FlutterResult
  }

  private weak var presenter: UIViewController?
  private var pendingRequests: [ExportRequest] = []
  private var activeRequest: ExportRequest?

  init(presenter: UIViewController) {
    self.presenter = presenter
    super.init()
  }

  /// Gestisce il metodo Flutter dedicato all'esportazione. Restituisce `false`
  /// per lasciare ad altri handler gli eventuali metodi non riconosciuti.
  func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) -> Bool {
    guard call.method == "clanto_export_received_file" else {
      return false
    }
    guard
      let arguments = call.arguments as? [String: Any],
      let path = arguments["path"] as? String,
      !path.isEmpty
    else {
      result(FlutterError(
        code: "invalid_arguments",
        message: "Percorso del file ricevuto non valido",
        details: nil
      ))
      return true
    }

    let sourceURL = URL(fileURLWithPath: path)
    guard FileManager.default.fileExists(atPath: sourceURL.path) else {
      result(FlutterError(
        code: "file_not_found",
        message: "Il file ricevuto non è disponibile",
        details: path
      ))
      return true
    }

    pendingRequests.append(ExportRequest(sourceURL: sourceURL, result: result))
    presentNextRequestIfNeeded()
    return true
  }

  private func presentNextRequestIfNeeded() {
    guard activeRequest == nil, !pendingRequests.isEmpty else {
      return
    }
    guard let presenter = presenter else {
      let request = pendingRequests.removeFirst()
      request.result(FlutterError(
        code: "presentation_unavailable",
        message: "Impossibile mostrare il selettore dell'app File",
        details: nil
      ))
      presentNextRequestIfNeeded()
      return
    }

    let request = pendingRequests.removeFirst()
    activeRequest = request
    let picker: UIDocumentPickerViewController
    if #available(iOS 14.0, *) {
      picker = UIDocumentPickerViewController(
        forExporting: [request.sourceURL],
        asCopy: true
      )
    } else {
      picker = UIDocumentPickerViewController(
        url: request.sourceURL,
        in: .exportToService
      )
    }
    picker.delegate = self
    picker.modalPresentationStyle = .formSheet
    presenter.present(picker, animated: true)
  }

  func documentPicker(
    _ controller: UIDocumentPickerViewController,
    didPickDocumentsAt urls: [URL]
  ) {
    finishActiveRequest(value: urls.first?.path ?? "")
  }

  func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
    // L'annullamento non invalida il trasferimento: la copia nel sandbox resta
    // disponibile tramite la cartella ClantoDesk dell'app File.
    finishActiveRequest(value: "")
  }

  private func finishActiveRequest(value: String) {
    guard let request = activeRequest else {
      return
    }
    activeRequest = nil
    request.result(value)
    DispatchQueue.main.async { [weak self] in
      self?.presentNextRequestIfNeeded()
    }
  }
}
