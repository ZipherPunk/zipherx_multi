/// TransactionHistoryView.swift
/// ZipherXSwift
///
/// Cypherpunk-styled transaction history list with neon accents.

import SwiftUI

@available(iOS 16, macOS 13, *)
public struct TransactionHistoryView: View {

    public let transactions: [WalletTransaction]
    public let sentCount: UInt32
    public let receivedCount: UInt32

    public init(transactions: [WalletTransaction], sentCount: UInt32 = 0, receivedCount: UInt32 = 0) {
        self.transactions = transactions
        self.sentCount = sentCount
        self.receivedCount = receivedCount
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("TRANSACTION HISTORY")
                    .font(ZFonts.caption)
                    .foregroundColor(ZColors.primaryDark)
                Spacer()
                Text("IN: \(receivedCount)")
                    .font(ZFonts.small)
                    .foregroundColor(ZColors.success)
                Text("OUT: \(sentCount)")
                    .font(ZFonts.small)
                    .foregroundColor(ZColors.error)
            }

            if transactions.isEmpty {
                emptyState
            } else {
                VStack(spacing: 0) {
                    ForEach(transactions) { tx in
                        TransactionRowView(transaction: tx)
                        Rectangle()
                            .fill(ZColors.primaryDim.opacity(0.3))
                            .frame(height: 1)
                    }
                }
                .background(ZColors.surface)
                .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 8) {
            Image(systemName: "tray")
                .font(.system(size: 28))
                .foregroundColor(ZColors.primaryDim)
            Text("No transactions yet.")
                .font(ZFonts.body)
                .foregroundColor(ZColors.primaryDim)
        }
        .frame(maxWidth: .infinity)
        .padding(32)
        .background(ZColors.surface)
        .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))
    }
}

// MARK: - TransactionRowView

@available(iOS 16, macOS 13, *)
private struct TransactionRowView: View {

    let transaction: WalletTransaction

    private var isSentType: Bool {
        transaction.txType == "sent" || transaction.txType.hasPrefix("alpha")
    }

    private var isReceivedType: Bool {
        transaction.txType == "received" || transaction.txType.hasPrefix("beta")
    }

    private var isSelfType: Bool {
        transaction.txType == "self"
    }

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: typeIcon)
                .font(ZFonts.heading)
                .foregroundColor(typeColor)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 2) {
                Text(typeLabel)
                    .font(ZFonts.body)
                    .foregroundColor(ZColors.primary)
                Text(confirmationLabel)
                    .font(ZFonts.small)
                    .foregroundColor(ZColors.primaryDim)
            }

            Spacer()

            VStack(alignment: .trailing, spacing: 2) {
                Text(amountFormatted)
                    .font(ZFonts.mono)
                    .foregroundColor(amountColor)
                Text(dateFormatted)
                    .font(ZFonts.small)
                    .foregroundColor(ZColors.primaryDim)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
    }

    private var typeIcon: String {
        if isSelfType { return "arrow.2.squarepath" }
        if isSentType { return "arrow.up.right" }
        if isReceivedType { return "arrow.down.left" }
        if transaction.txType == "change" { return "arrow.2.squarepath" }
        return "questionmark.circle"
    }

    private var typeColor: Color {
        if isSelfType { return ZColors.warning }
        if isSentType { return ZColors.error }
        if isReceivedType { return ZColors.success }
        if transaction.txType == "change" { return ZColors.warning }
        return ZColors.primaryDim
    }

    private var typeLabel: String {
        switch transaction.txType {
        case "self":     return "Self"
        case "sent":     return "Sent"
        case "received": return "Received"
        case "change":   return "Change"
        default:         return transaction.txType.capitalized
        }
    }

    private var amountColor: Color {
        if isSelfType { return ZColors.warning }
        if isReceivedType { return ZColors.success }
        return ZColors.primary
    }

    private var amountFormatted: String {
        let zcl = Double(transaction.amount) / 1e8
        let prefix = (isSentType || isSelfType) ? "-" : "+"
        return String(format: "%@%.8f ZCL", prefix, zcl)
    }

    private var confirmationLabel: String {
        switch transaction.confirmations {
        case 0:  return "Unconfirmed"
        case 1:  return "1 confirmation"
        default: return "\(transaction.confirmations) confirmations"
        }
    }

    private var dateFormatted: String {
        guard transaction.timestamp > 0 else { return "Pending" }
        let date = Date(timeIntervalSince1970: Double(transaction.timestamp))
        let fmt = DateFormatter()
        fmt.dateStyle = .short
        fmt.timeStyle = .short
        return fmt.string(from: date)
    }
}
