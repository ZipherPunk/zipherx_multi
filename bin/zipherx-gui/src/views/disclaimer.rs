//! Disclaimer screen — must scroll to bottom before accepting.

use zipherx_platform::SecureStorage;

use crate::app::{Phase, ZipherXApp};
use crate::theme;

const DISCLAIMER_TEXT: &str = r#"ZIPHERX — IMPORTANT LEGAL NOTICE

1. OPEN SOURCE SOFTWARE

ZipherX is free, open-source software distributed under the MIT License. This application is a tool that enables users to interact with the Zclassic blockchain network. The software is provided "as is" without any representations or warranties of any kind, either express or implied.

2. PRIVACY AS A FUNDAMENTAL RIGHT

Privacy is a fundamental human right recognized by the United Nations Declaration of Human Rights, the International Covenant on Civil and Political Rights, and numerous other international and regional treaties. ZipherX implements cryptographic privacy features that exist to protect this fundamental right. Financial privacy is essential for personal security, protection from discrimination, and the preservation of human dignity.

3. NON-CUSTODIAL ARCHITECTURE

ZipherX is a non-custodial wallet. The developer(s) of this software:

• Have NO access to your private keys or funds
• Cannot freeze, seize, or control your assets
• Cannot reverse, cancel, or modify any transactions
• Do NOT collect, store, or transmit any personal data
• Do NOT operate any central servers or maintain any logs

Your keys are stored exclusively on your device using hardware-backed encryption.

4. DECENTRALIZED NETWORK

ZipherX connects directly to the peer-to-peer Zclassic network. There is no central server, no intermediary, and no single point of control. The software is merely an interface to interact with a decentralized, permissionless blockchain network that operates independently of any individual or organization.

5. USER RESPONSIBILITY

By using this software, you acknowledge and agree that:

• YOU are solely responsible for compliance with all applicable laws and regulations in your jurisdiction
• YOU are responsible for securing your recovery phrase and private keys
• YOU are responsible for verifying transaction details before confirmation
• YOU understand that blockchain transactions are irreversible
• YOU accept all risks associated with using cryptocurrency software

6. NO FINANCIAL ADVICE

Nothing in this software constitutes financial, investment, legal, or tax advice. The developer(s) are not financial advisors. You should consult qualified professionals for any financial decisions. Cryptocurrency values are volatile and you may lose some or all of your investment.

7. LIMITATION OF LIABILITY

TO THE MAXIMUM EXTENT PERMITTED BY APPLICABLE LAW, IN NO EVENT SHALL THE DEVELOPERS, CONTRIBUTORS, OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES, OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT, OR OTHERWISE, ARISING FROM, OUT OF, OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

This includes but is not limited to: loss of funds, loss of profits, loss of data, business interruption, or any indirect, incidental, special, or consequential damages.

8. INTENDED USE

This software is intended for legitimate privacy-preserving financial transactions. Legitimate uses include but are not limited to:

• Protecting personal financial information from data breaches
• Preventing financial surveillance and profiling
• Protecting business confidentiality
• Donations to sensitive causes (journalism, activism, charity)
• Personal security in high-risk environments

The existence of privacy tools does not imply endorsement of any illegal activity.

9. JURISDICTIONAL NOTICE

Cryptocurrency regulations vary by jurisdiction. Some features of this software may not be legal in all jurisdictions. It is YOUR responsibility to ensure that your use of this software complies with all applicable laws in your location. The developer(s) make no representations regarding the legality of this software in any jurisdiction.

10. EXPERIMENTAL SOFTWARE

ZipherX is beta software under active development. It may contain bugs, errors, defects, or incomplete features that could result in loss of funds, corrupted data, or unexpected behavior. There is NO guarantee that the software will function correctly, continuously, or without interruption. DO NOT use this software with funds you cannot afford to lose entirely and permanently.

11. INDEMNIFICATION

BY USING THIS SOFTWARE, YOU AGREE TO INDEMNIFY, DEFEND, AND HOLD HARMLESS THE DEVELOPERS, CONTRIBUTORS, AND COPYRIGHT HOLDERS FROM AND AGAINST ANY AND ALL CLAIMS, LIABILITIES, DAMAGES, LOSSES, COSTS, AND EXPENSES (INCLUDING REASONABLE LEGAL FEES) ARISING OUT OF OR RELATED TO YOUR USE OR MISUSE OF THIS SOFTWARE, YOUR VIOLATION OF THIS DISCLAIMER, OR YOUR VIOLATION OF ANY APPLICABLE LAW OR REGULATION.

12. THIRD-PARTY SERVICES & FORCE MAJEURE

ZipherX relies on third-party decentralized services including but not limited to: the Zclassic blockchain network, the Tor anonymity network, and peer-to-peer node operators. The developer(s) have NO control over these networks and accept NO responsibility for:

• Network outages, congestion, or failures
• Blockchain forks, reorganizations, or protocol changes
• Tor network disruptions or de-anonymization attacks
• Malicious peer nodes or Sybil attacks
• Acts of God, war, government action, or any event beyond reasonable control

Your use of these third-party networks is entirely at your own risk.

13. BACKUP WARNING

YOU MUST BACK UP YOUR WALLET BEFORE INSTALLING OR USING ZIPHERX. If you are running an existing Zclassic full node or any other wallet software, back up ALL wallet files, private keys, and spending keys BEFORE proceeding. ZipherX's Full Node mode connects to your local node -- software bugs could potentially overwrite, corrupt, or delete existing wallet data. This applies to both P2P mode and Full Node mode. The developer(s) accept NO responsibility for loss of funds or data resulting from failure to maintain adequate backups. ALWAYS maintain independent, offline backups of your keys and wallet files. Never rely solely on any single piece of software to protect your funds.

14. VOLUNTARY CONTRIBUTIONS

All contributions to the development of ZipherX -- including but not limited to code, documentation, design, testing, bug reports, translations, and feedback -- are made on a strictly voluntary and unpaid basis. Contributing to this project does NOT entitle any contributor to:

• Any form of compensation, payment, or remuneration
• Any ownership, equity, or intellectual property rights in the software
• Any share of revenue, profits, donations, or financial benefits
• Any employment, contractor, or business relationship with the developer(s)
• Any decision-making authority over the project's direction

Contributions are made under the terms of the MIT License. By contributing, you agree that your contributions become part of the open-source project with no expectation of compensation or ownership of any kind, now or in the future.


"Privacy is necessary for an open society in the electronic age. Privacy is not secrecy. A private matter is something one doesn't want the whole world to know, but a secret matter is something one doesn't want anybody to know. Privacy is the power to selectively reveal oneself to the world."
-- Eric Hughes, A Cypherpunk's Manifesto (1993)


BY PROCEEDING, YOU ACKNOWLEDGE THAT:

• You have read and understood all 14 sections of this disclaimer
• You are at least 18 years of age or the age of majority in your jurisdiction
• You accept full responsibility for your use of this software
• You will comply with all applicable laws in your jurisdiction
• You understand the risks of using cryptocurrency and beta software
• You agree to the indemnification terms in Section 11
• You understand that third-party networks are outside the developer's control
• You have backed up all existing wallet files and keys before using this software"#;

pub fn show(app: &mut ZipherXApp, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(
                egui::RichText::new("ZIPHERX")
                    .font(theme::mono(24.0))
                    .color(theme::GREEN),
            );
            ui.add_space(10.0);
        });

        let available = ui.available_height() - 60.0;
        let scroll_area = egui::ScrollArea::vertical()
            .max_height(available)
            .auto_shrink([false; 2]);

        let response = scroll_area.show(ui, |ui| {
            ui.label(
                egui::RichText::new(DISCLAIMER_TEXT)
                    .font(theme::mono(12.0))
                    .color(theme::MUTED),
            );
            // Invisible marker at the bottom to detect scroll completion
            ui.label("");
        });

        // Check if scrolled near the bottom
        let offset = response.state.offset.y;
        let content_height = response.content_size.y;
        let viewport_height = available;
        if content_height > viewport_height {
            if offset + viewport_height >= content_height - 20.0 {
                app.disclaimer_scrolled_to_bottom = true;
            }
        } else {
            // Content fits without scrolling
            app.disclaimer_scrolled_to_bottom = true;
        }

        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            let enabled = app.disclaimer_scrolled_to_bottom;
            let btn = egui::Button::new(
                egui::RichText::new(if enabled {
                    "[ I ACCEPT AND UNDERSTAND ]"
                } else {
                    "[ SCROLL TO BOTTOM TO ACCEPT ]"
                })
                .font(theme::mono(14.0))
                .color(if enabled { theme::GREEN } else { theme::MUTED }),
            );

            if ui.add_enabled(enabled, btn).clicked() {
                // Persist acceptance
                let marker = app.data_dir.join(".disclaimer_accepted");
                let _ = std::fs::write(&marker, "1");

                // Transition to password/setup
                if app.storage.has_key("spending_key") {
                    app.phase = Phase::Locked;
                } else {
                    app.phase = Phase::Locked; // Need password first
                }
            }
        });
    });
}
