/// DisclaimerView.swift
/// ZipherXSwift
///
/// 14-section legal disclaimer with cypherpunk terminal aesthetic.
/// Accept button is disabled until user scrolls to the bottom.

import SwiftUI

@available(iOS 17, macOS 14, *)
public struct DisclaimerView: View {

    var onAccept: () -> Void

    @State private var hasScrolledToBottom = false

    public init(onAccept: @escaping () -> Void) {
        self.onAccept = onAccept
    }

    public var body: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()

            VStack(spacing: 0) {
                // Scrollable content
                ScrollView {
                    VStack(spacing: 0) {
                        disclaimerContent
                            .padding(24)

                        // Invisible anchor at the very bottom for scroll detection
                        GeometryReader { geo in
                            Color.clear
                                .onAppear {
                                    checkScrollPosition(geo)
                                }
                                .onChange(of: geo.frame(in: .global).minY) { _, _ in
                                    checkScrollPosition(geo)
                                }
                        }
                        .frame(height: 1)
                    }
                }

                // Fixed bottom bar
                bottomBar
            }
        }
        .foregroundColor(ZColors.primary)
    }

    // MARK: - Scroll Detection

    private func checkScrollPosition(_ geo: GeometryProxy) {
        let screenHeight: CGFloat
        #if os(iOS)
        screenHeight = UIScreen.main.bounds.height
        #else
        screenHeight = NSScreen.main?.frame.height ?? 800
        #endif
        // When the bottom anchor is visible on screen, user has scrolled far enough
        if geo.frame(in: .global).minY < screenHeight + 50 {
            hasScrolledToBottom = true
        }
    }

    // MARK: - Disclaimer Content

    private var disclaimerContent: some View {
        VStack(spacing: 12) {
            // Header
            Image(systemName: "lock.shield.fill")
                .font(.system(size: 50))
                .foregroundColor(ZColors.primary)
                .shadow(color: ZColors.primary.opacity(0.5), radius: 8)

            Text("ZIPHERX")
                .font(.system(size: 24, weight: .bold, design: .monospaced))
                .foregroundColor(ZColors.primary)
                .tracking(4)

            Text("IMPORTANT LEGAL NOTICE")
                .font(ZFonts.heading)
                .foregroundColor(ZColors.primary.opacity(0.8))

            Spacer().frame(height: 8)

            // Section 1
            disclaimerSection(
                title: "1. OPEN SOURCE SOFTWARE",
                content: "ZipherX is free, open-source software distributed under the MIT License. "
                    + "This application is a tool that enables users to interact with the Zclassic blockchain network. "
                    + "The software is provided \"as is\" without any representations or warranties of any kind, "
                    + "either express or implied."
            )

            // Section 2
            disclaimerSection(
                title: "2. PRIVACY AS A FUNDAMENTAL RIGHT",
                content: "Privacy is a fundamental human right recognized by the United Nations Declaration of Human Rights, "
                    + "the International Covenant on Civil and Political Rights, and numerous other international and regional treaties. "
                    + "ZipherX implements cryptographic privacy features that exist to protect this fundamental right. "
                    + "Financial privacy is essential for personal security, protection from discrimination, "
                    + "and the preservation of human dignity."
            )

            // Section 3
            disclaimerSection(
                title: "3. NON-CUSTODIAL ARCHITECTURE",
                content: "ZipherX is a non-custodial wallet. The developer(s) of this software:\n\n"
                    + "\u{2022} Have NO access to your private keys or funds\n"
                    + "\u{2022} Cannot freeze, seize, or control your assets\n"
                    + "\u{2022} Cannot reverse, cancel, or modify any transactions\n"
                    + "\u{2022} Do NOT collect, store, or transmit any personal data\n"
                    + "\u{2022} Do NOT operate any central servers or maintain any logs\n\n"
                    + "Your keys are stored exclusively on your device using hardware-backed encryption."
            )

            // Section 4
            disclaimerSection(
                title: "4. DECENTRALIZED NETWORK",
                content: "ZipherX connects directly to the peer-to-peer Zclassic network. "
                    + "There is no central server, no intermediary, and no single point of control. "
                    + "The software is merely an interface to interact with a decentralized, "
                    + "permissionless blockchain network that operates independently of any individual or organization."
            )

            // Section 5
            disclaimerSection(
                title: "5. USER RESPONSIBILITY",
                content: "By using this software, you acknowledge and agree that:\n\n"
                    + "\u{2022} YOU are solely responsible for compliance with all applicable laws and regulations in your jurisdiction\n"
                    + "\u{2022} YOU are responsible for securing your recovery phrase and private keys\n"
                    + "\u{2022} YOU are responsible for verifying transaction details before confirmation\n"
                    + "\u{2022} YOU understand that blockchain transactions are irreversible\n"
                    + "\u{2022} YOU accept all risks associated with using cryptocurrency software"
            )

            // Section 6
            disclaimerSection(
                title: "6. NO FINANCIAL ADVICE",
                content: "Nothing in this software constitutes financial, investment, legal, or tax advice. "
                    + "The developer(s) are not financial advisors. "
                    + "You should consult qualified professionals for any financial decisions. "
                    + "Cryptocurrency values are volatile and you may lose some or all of your investment."
            )

            // Section 7
            disclaimerSection(
                title: "7. LIMITATION OF LIABILITY",
                content: "TO THE MAXIMUM EXTENT PERMITTED BY APPLICABLE LAW, IN NO EVENT SHALL THE DEVELOPERS, "
                    + "CONTRIBUTORS, OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES, OR OTHER LIABILITY, "
                    + "WHETHER IN AN ACTION OF CONTRACT, TORT, OR OTHERWISE, ARISING FROM, OUT OF, "
                    + "OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.\n\n"
                    + "This includes but is not limited to: loss of funds, loss of profits, loss of data, "
                    + "business interruption, or any indirect, incidental, special, or consequential damages."
            )

            // Section 8
            disclaimerSection(
                title: "8. INTENDED USE",
                content: "This software is intended for legitimate privacy-preserving financial transactions. "
                    + "Legitimate uses include but are not limited to:\n\n"
                    + "\u{2022} Protecting personal financial information from data breaches\n"
                    + "\u{2022} Preventing financial surveillance and profiling\n"
                    + "\u{2022} Protecting business confidentiality\n"
                    + "\u{2022} Donations to sensitive causes (journalism, activism, charity)\n"
                    + "\u{2022} Personal security in high-risk environments\n\n"
                    + "The existence of privacy tools does not imply endorsement of any illegal activity."
            )

            // Section 9
            disclaimerSection(
                title: "9. JURISDICTIONAL NOTICE",
                content: "Cryptocurrency regulations vary by jurisdiction. Some features of this software may not be "
                    + "legal in all jurisdictions. It is YOUR responsibility to ensure that your use of this "
                    + "software complies with all applicable laws in your location. "
                    + "The developer(s) make no representations regarding the legality of this software in any jurisdiction."
            )

            // Section 10
            disclaimerSection(
                title: "10. EXPERIMENTAL SOFTWARE",
                content: "ZipherX is beta software under active development. It may contain bugs, errors, "
                    + "defects, or incomplete features that could result in loss of funds, corrupted data, "
                    + "or unexpected behavior. There is NO guarantee that the software will function correctly, "
                    + "continuously, or without interruption. "
                    + "DO NOT use this software with funds you cannot afford to lose entirely and permanently."
            )

            // Section 11
            disclaimerSection(
                title: "11. INDEMNIFICATION",
                content: "BY USING THIS SOFTWARE, YOU AGREE TO INDEMNIFY, DEFEND, AND HOLD HARMLESS THE DEVELOPERS, "
                    + "CONTRIBUTORS, AND COPYRIGHT HOLDERS FROM AND AGAINST ANY AND ALL CLAIMS, LIABILITIES, "
                    + "DAMAGES, LOSSES, COSTS, AND EXPENSES (INCLUDING REASONABLE LEGAL FEES) ARISING OUT OF OR "
                    + "RELATED TO YOUR USE OR MISUSE OF THIS SOFTWARE, YOUR VIOLATION OF THIS DISCLAIMER, "
                    + "OR YOUR VIOLATION OF ANY APPLICABLE LAW OR REGULATION."
            )

            // Section 12
            disclaimerSection(
                title: "12. THIRD-PARTY SERVICES & FORCE MAJEURE",
                content: "ZipherX relies on third-party decentralized services including but not limited to: "
                    + "the Zclassic blockchain network, the Tor anonymity network, and peer-to-peer node operators. "
                    + "The developer(s) have NO control over these networks and accept NO responsibility for:\n\n"
                    + "\u{2022} Network outages, congestion, or failures\n"
                    + "\u{2022} Blockchain forks, reorganizations, or protocol changes\n"
                    + "\u{2022} Tor network disruptions or de-anonymization attacks\n"
                    + "\u{2022} Malicious peer nodes or Sybil attacks\n"
                    + "\u{2022} Acts of God, war, government action, or any event beyond reasonable control\n\n"
                    + "Your use of these third-party networks is entirely at your own risk."
            )

            // Section 13
            disclaimerSection(
                title: "13. BACKUP WARNING",
                content: "YOU MUST BACK UP YOUR WALLET BEFORE INSTALLING OR USING ZIPHERX. "
                    + "If you are running an existing Zclassic full node or any other wallet software, "
                    + "back up ALL wallet files, private keys, and spending keys BEFORE proceeding. "
                    + "The developer(s) accept NO responsibility for loss of funds or data "
                    + "resulting from failure to maintain adequate backups. "
                    + "ALWAYS maintain independent, offline backups of your keys and wallet files. "
                    + "Never rely solely on any single piece of software to protect your funds."
            )

            // Section 14
            disclaimerSection(
                title: "14. VOLUNTARY CONTRIBUTIONS",
                content: "All contributions to the development of ZipherX \u{2014} including but not limited to code, "
                    + "documentation, design, testing, bug reports, translations, and feedback \u{2014} are made "
                    + "on a strictly voluntary and unpaid basis. Contributing to this project does NOT entitle "
                    + "any contributor to any form of compensation, ownership, equity, or decision-making authority."
            )

            Spacer().frame(height: 12)

            // Cypherpunk quote
            cypherpunkQuote

            Spacer().frame(height: 12)

            // Acknowledgment checklist
            acknowledgmentChecklist

            Spacer().frame(height: 16)
        }
    }

    // MARK: - Disclaimer Section

    private func disclaimerSection(title: String, content: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(ZFonts.body)
                .fontWeight(.bold)
                .foregroundColor(ZColors.primary)

            Text(content)
                .font(ZFonts.caption)
                .foregroundColor(.white.opacity(0.85))
                .lineSpacing(4)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(ZColors.primary.opacity(0.05))
        .overlay(Rectangle().stroke(ZColors.primary.opacity(0.2), lineWidth: 1))
    }

    // MARK: - Cypherpunk Quote

    private var cypherpunkQuote: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("\"Privacy is necessary for an open society in the electronic age. Privacy is not secrecy. "
                + "A private matter is something one doesn't want the whole world to know, but a secret matter "
                + "is something one doesn't want anybody to know. Privacy is the power to selectively reveal oneself to the world.\"")
                .font(ZFonts.caption)
                .foregroundColor(ZColors.primary.opacity(0.9))
                .lineSpacing(4)
                .fixedSize(horizontal: false, vertical: true)

            Text("- Eric Hughes, A Cypherpunk's Manifesto (1993)")
                .font(ZFonts.small)
                .fontWeight(.semibold)
                .foregroundColor(ZColors.primary.opacity(0.7))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(ZColors.primary.opacity(0.08))
    }

    // MARK: - Acknowledgment Checklist

    private var acknowledgmentChecklist: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("BY PROCEEDING, YOU ACKNOWLEDGE THAT:")
                .font(ZFonts.caption)
                .fontWeight(.bold)
                .foregroundColor(.white)

            ackItem("You have read and understood all 14 sections of this disclaimer")
            ackItem("You are at least 18 years of age or the age of majority in your jurisdiction")
            ackItem("You accept full responsibility for your use of this software")
            ackItem("You will comply with all applicable laws in your jurisdiction")
            ackItem("You understand the risks of using cryptocurrency and beta software")
            ackItem("You agree to the indemnification terms in Section 11")
            ackItem("You have backed up all existing wallet files and keys before using this software")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(Color.white.opacity(0.05))
        .overlay(Rectangle().stroke(Color.white.opacity(0.2), lineWidth: 1))
    }

    private func ackItem(_ text: String) -> some View {
        HStack(alignment: .top, spacing: 0) {
            Text("> ")
                .font(ZFonts.small)
                .fontWeight(.bold)
                .foregroundColor(ZColors.primary)

            Text(text)
                .font(ZFonts.small)
                .foregroundColor(.white.opacity(0.8))
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    // MARK: - Bottom Bar

    private var bottomBar: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(ZColors.primary.opacity(0.3))
                .frame(height: 1)

            VStack(spacing: 8) {
                if !hasScrolledToBottom {
                    Text("Scroll down to read the entire disclaimer...")
                        .font(ZFonts.small)
                        .foregroundColor(ZColors.primary.opacity(0.6))
                }

                Button(action: onAccept) {
                    Text("I ACCEPT AND UNDERSTAND")
                        .font(ZFonts.body)
                        .fontWeight(.bold)
                        .foregroundColor(hasScrolledToBottom ? ZColors.primary : ZColors.primaryDim.opacity(0.5))
                        .padding(.vertical, 10)
                        .frame(maxWidth: .infinity)
                        .overlay(
                            Rectangle()
                                .stroke(
                                    hasScrolledToBottom ? ZColors.primary : ZColors.primary.opacity(0.3),
                                    lineWidth: 1
                                )
                        )
                }
                .buttonStyle(.plain)
                .disabled(!hasScrolledToBottom)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 16)
            .background(ZColors.terminalBlack)
        }
    }
}
