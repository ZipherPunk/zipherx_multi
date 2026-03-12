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

    @State private var selectedTransaction: WalletTransaction?

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
                            .contentShape(Rectangle())
                            .onTapGesture {
                                selectedTransaction = tx
                            }
                        Rectangle()
                            .fill(ZColors.primaryDim.opacity(0.3))
                            .frame(height: 1)
                    }
                }
                .background(ZColors.surface)
                .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))
            }
        }
        .sheet(item: $selectedTransaction) { tx in
            TransactionDetailSheet(transaction: tx)
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

// MARK: - Transaction Detail Sheet

@available(iOS 16, macOS 13, *)
private struct TransactionDetailSheet: View {

    let transaction: WalletTransaction
    @Environment(\.dismiss) private var dismiss
    @State private var copiedTxid = false

    private var isSent: Bool {
        transaction.txType == "sent" || transaction.txType.hasPrefix("alpha")
    }
    private var isReceived: Bool {
        transaction.txType == "received" || transaction.txType.hasPrefix("beta")
    }
    private var isSelf: Bool {
        transaction.txType == "self"
    }

    private var typeLabel: String {
        if isSelf { return "SELF TRANSFER" }
        if isReceived { return "RECEIVED" }
        if isSent { return "SENT" }
        return transaction.txType.uppercased()
    }

    private var typeColor: Color {
        if isSelf { return ZColors.warning }
        if isReceived { return ZColors.success }
        if isSent { return ZColors.error }
        return ZColors.primaryDim
    }

    var body: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()

            VStack(alignment: .leading, spacing: 0) {
                // Header
                HStack {
                    Text("> TRANSACTION DETAILS")
                        .font(ZFonts.caption)
                        .foregroundColor(typeColor)
                    Spacer()
                    Button(action: { dismiss() }) {
                        Text("CLOSE")
                            .font(ZFonts.small)
                            .foregroundColor(ZColors.primaryDim)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 16)
                .padding(.top, 20)
                .padding(.bottom, 12)

                // Detail card
                VStack(alignment: .leading, spacing: 0) {
                    detailRow(label: "TYPE", value: typeLabel, valueColor: typeColor)

                    let zcl = Double(transaction.amount) / 1e8
                    detailRow(label: "AMOUNT", value: String(format: "%.8f ZCL", zcl))

                    if transaction.fee > 0 {
                        let feeZcl = Double(transaction.fee) / 1e8
                        detailRow(label: "FEE", value: String(format: "%.8f ZCL", feeZcl))
                    }

                    detailRow(label: "CONFIRMATIONS", value: "\(transaction.confirmations)")

                    detailRow(label: "BLOCK HEIGHT", value: transaction.height > 0 ? "\(transaction.height)" : "Pending")

                    let dateStr: String = {
                        guard transaction.timestamp > 0 else { return "Unknown" }
                        let date = Date(timeIntervalSince1970: Double(transaction.timestamp))
                        let fmt = DateFormatter()
                        fmt.dateFormat = "yyyy-MM-dd HH:mm:ss"
                        return fmt.string(from: date)
                    }()
                    detailRow(label: "DATE", value: dateStr)

                    if let memo = transaction.memo, !memo.isEmpty {
                        detailRow(label: "MEMO", value: memo)
                    }

                    // Divider
                    Rectangle()
                        .fill(ZColors.primaryDim.opacity(0.3))
                        .frame(height: 1)
                        .padding(.vertical, 8)

                    // TXID section
                    Text("TXID")
                        .font(.system(size: 9, weight: .regular, design: .monospaced))
                        .foregroundColor(ZColors.primaryDim)
                        .tracking(1)
                        .padding(.bottom, 4)

                    HStack(alignment: .center, spacing: 8) {
                        Text(transaction.txid)
                            .font(.system(size: 8, weight: .regular, design: .monospaced))
                            .foregroundColor(ZColors.primary)
                            .lineLimit(nil)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)

                        Spacer(minLength: 4)

                        Button(action: copyTxid) {
                            HStack(spacing: 4) {
                                Image(systemName: copiedTxid ? "checkmark" : "doc.on.doc")
                                    .font(.system(size: 12))
                                Text(copiedTxid ? "COPIED" : "COPY")
                                    .font(.system(size: 9, weight: .regular, design: .monospaced))
                            }
                            .foregroundColor(copiedTxid ? ZColors.success : ZColors.primary)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .overlay(Rectangle().stroke(copiedTxid ? ZColors.success : ZColors.primaryDim, lineWidth: 1))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(12)
                .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))
                .padding(.horizontal, 16)

                Spacer()
            }
        }
        .presentationDetents([.medium])
        .presentationDragIndicator(.visible)
    }

    @ViewBuilder
    private func detailRow(label: String, value: String, valueColor: Color = ZColors.primary) -> some View {
        HStack {
            Text(label)
                .font(.system(size: 9, weight: .regular, design: .monospaced))
                .foregroundColor(ZColors.primaryDim)
                .tracking(1)
            Spacer()
            Text(value)
                .font(.system(size: 10, weight: .semibold, design: .monospaced))
                .foregroundColor(valueColor)
                .multilineTextAlignment(.trailing)
        }
        .padding(.vertical, 3)
    }

    private func copyTxid() {
        #if os(iOS)
        UIPasteboard.general.string = transaction.txid
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(transaction.txid, forType: .string)
        #endif

        withAnimation { copiedTxid = true }

        // Reset "COPIED" label after 2 seconds
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            withAnimation { copiedTxid = false }
        }

        // Auto-clear clipboard after 30 seconds for security
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            #if os(iOS)
            if UIPasteboard.general.string == transaction.txid {
                UIPasteboard.general.string = ""
            }
            #elseif os(macOS)
            if NSPasteboard.general.string(forType: .string) == transaction.txid {
                NSPasteboard.general.clearContents()
            }
            #endif
        }
    }
}
