import Foundation
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// System clipboard abstraction for iOS and macOS.
public final class AppleClipboard: @unchecked Sendable {

    public init() {}

    /// Copy `text` to the system clipboard.
    /// SA-3: Auto-clears clipboard after 30 seconds to prevent sensitive data leakage.
    public func copyText(_ text: String) {
        #if canImport(UIKit)
        UIPasteboard.general.string = text
        #elseif canImport(AppKit)
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
        #endif

        // SA-3: Auto-clear clipboard after 30 seconds
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            #if canImport(UIKit)
            if UIPasteboard.general.string == text {
                UIPasteboard.general.string = ""
            }
            #elseif canImport(AppKit)
            if NSPasteboard.general.string(forType: .string) == text {
                NSPasteboard.general.clearContents()
            }
            #endif
        }
    }

    /// Return the current string content of the system clipboard, or `nil` if empty.
    public func pasteText() -> String? {
        #if canImport(UIKit)
        return UIPasteboard.general.string
        #elseif canImport(AppKit)
        return NSPasteboard.general.string(forType: .string)
        #else
        return nil
        #endif
    }
}
