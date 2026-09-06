import AppKit
import ApplicationServices
import Foundation

guard CommandLine.arguments.count == 2, let pid = Int32(CommandLine.arguments[1]),
      let app = NSRunningApplication(processIdentifier: pid),
      let bundle = app.bundleIdentifier, bundle.hasPrefix("me.grab.hide."),
      AXIsProcessTrusted() else {
    fatalError("A running isolated development bundle and Accessibility permission are required")
}
func attribute(_ element: AXUIElement, _ name: String) -> AnyObject? {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    guard result == .success || result == .noValue || result == .attributeUnsupported else {
        fatalError("Accessibility read failed: \(name), \(result.rawValue)")
    }
    return value
}
var rows: [[String: String]] = []
var visited: Set<CFHashCode> = []
func visit(_ element: AXUIElement) {
    guard visited.insert(CFHash(element)).inserted else { return }
    if let help = attribute(element, kAXHelpAttribute) as? String {
        let label = attribute(element, kAXDescriptionAttribute) as? String
            ?? attribute(element, kAXTitleAttribute) as? String ?? ""
        rows.append(["label": label, "help": help])
    }
    for child in attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? [] { visit(child) }
}
visit(AXUIElementCreateApplication(pid))
let data = try JSONSerialization.data(withJSONObject: ["pid": pid, "bundle": bundle, "controls": rows], options: [.prettyPrinted, .sortedKeys])
print(String(decoding: data, as: UTF8.self))
