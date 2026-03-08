/// ZipherXTheme.swift
/// ZipherXSwift
///
/// Cypherpunk terminal theme matching the original ZipherX design.
/// Neon orange on dark terminal background, monospaced fonts, sharp corners.

import SwiftUI

// MARK: - Theme Colors

public enum ZColors {
    // Primary accent (macOS = orange, iOS = green)
    #if os(macOS)
    public static let primary      = Color(red: 1.0, green: 0.4, blue: 0.0)   // #FF6600
    public static let primaryDark  = Color(red: 0.8, green: 0.3, blue: 0.0)   // #CC4D00
    public static let primaryDim   = Color(red: 0.5, green: 0.2, blue: 0.0)   // #803300
    public static let success      = Color(red: 0.2, green: 0.8, blue: 0.2)   // #33CC33
    #else
    public static let primary      = Color(red: 0, green: 1, blue: 0.25)      // #00FF40
    public static let primaryDark  = Color(red: 0, green: 0.85, blue: 0.25)   // #00D940
    public static let primaryDim   = Color(red: 0, green: 0.7, blue: 0.18)    // #00B32E
    public static let success      = Color(red: 0, green: 1, blue: 0.25)
    #endif

    // Backgrounds
    public static let terminalDark  = Color(red: 0.06, green: 0.04, blue: 0.02) // #0F0A05
    public static let terminalBlack = Color(red: 0.02, green: 0.02, blue: 0.02) // #050505
    public static let surface       = Color(red: 0.10, green: 0.07, blue: 0.03) // #1A1208
    public static let progressBg    = Color(red: 0.15, green: 0.10, blue: 0.02) // #261905

    // Status
    public static let error   = Color.red
    public static let warning = Color.yellow

    // Glow
    public static let glow = primary.opacity(0.3)
}

// MARK: - Theme Fonts

public enum ZFonts {
    #if os(macOS)
    public static let title   = Font.system(size: 18, weight: .bold, design: .monospaced)
    public static let heading = Font.system(size: 15, weight: .semibold, design: .monospaced)
    public static let body    = Font.system(size: 14, weight: .regular, design: .monospaced)
    public static let mono    = Font.system(size: 13, weight: .regular, design: .monospaced)
    public static let caption = Font.system(size: 12, weight: .regular, design: .monospaced)
    public static let small   = Font.system(size: 10, weight: .regular, design: .monospaced)
    public static let balance = Font.system(size: 28, weight: .bold, design: .monospaced)
    #else
    public static let title   = Font.system(size: 14, weight: .bold, design: .monospaced)
    public static let heading = Font.system(size: 13, weight: .semibold, design: .monospaced)
    public static let body    = Font.system(size: 12, weight: .regular, design: .monospaced)
    public static let mono    = Font.system(size: 11, weight: .regular, design: .monospaced)
    public static let caption = Font.system(size: 10, weight: .regular, design: .monospaced)
    public static let small   = Font.system(size: 9, weight: .regular, design: .monospaced)
    public static let balance = Font.system(size: 24, weight: .bold, design: .monospaced)
    #endif
}

// MARK: - Themed Components

public struct ZCard<Content: View>: View {
    let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        content
            .padding(16)
            .background(ZColors.surface)
            .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))
            .shadow(color: ZColors.glow, radius: 3)
    }
}

public struct ZButton: View {
    let title: String
    let icon: String?
    let style: Style
    let action: () -> Void

    public enum Style { case primary, secondary, danger }

    public init(_ title: String, icon: String? = nil, style: Style = .primary, action: @escaping () -> Void) {
        self.title = title
        self.icon = icon
        self.style = style
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if let icon {
                    Image(systemName: icon)
                        .font(ZFonts.body)
                }
                Text(title.uppercased())
                    .font(ZFonts.body)
            }
            .foregroundColor(foregroundColor)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity)
            .background(backgroundColor)
            .overlay(Rectangle().stroke(borderColor, lineWidth: 1))
            .shadow(color: glowColor, radius: 2, y: 1)
        }
        .buttonStyle(.plain)
    }

    private var foregroundColor: Color {
        switch style {
        case .primary: return ZColors.terminalBlack
        case .secondary: return ZColors.primary
        case .danger: return ZColors.error
        }
    }

    private var backgroundColor: Color {
        switch style {
        case .primary: return ZColors.primary
        case .secondary: return ZColors.surface
        case .danger: return ZColors.surface
        }
    }

    private var borderColor: Color {
        switch style {
        case .primary: return ZColors.primary
        case .secondary: return ZColors.primaryDim
        case .danger: return ZColors.error.opacity(0.5)
        }
    }

    private var glowColor: Color {
        switch style {
        case .primary: return ZColors.primary.opacity(0.3)
        case .secondary: return ZColors.primaryDim.opacity(0.2)
        case .danger: return ZColors.error.opacity(0.2)
        }
    }
}

public struct ZTextField: View {
    let placeholder: String
    @Binding var text: String

    public init(_ placeholder: String, text: Binding<String>) {
        self.placeholder = placeholder
        self._text = text
    }

    public var body: some View {
        TextField(placeholder, text: $text)
            .font(ZFonts.mono)
            .foregroundColor(ZColors.primary)
            .padding(10)
            .background(ZColors.terminalBlack)
            .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))
            #if os(iOS)
            .textInputAutocapitalization(.never)
            #endif
            .autocorrectionDisabled()
    }
}

public struct ZProgressBar: View {
    let progress: Double

    public init(progress: Double) {
        self.progress = progress
    }

    public var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Rectangle()
                    .fill(ZColors.progressBg)
                    .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))

                Rectangle()
                    .fill(
                        LinearGradient(
                            colors: [ZColors.primaryDark, ZColors.primary],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: max(0, geo.size.width * progress - 4))
                    .padding(2)
                    .shadow(color: ZColors.glow, radius: 2)
            }
        }
        .frame(height: 12)
    }
}
