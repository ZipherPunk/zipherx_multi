package com.zipherx.wallet.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import com.zipherx.wallet.R
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.ZColors

@Composable
fun DisclaimerScreen(
    onAccept: () -> Unit,
) {
    val scrollState = rememberScrollState()
    var hasScrolledToBottom by remember { mutableStateOf(false) }

    // Detect when user reaches the bottom (within 50px tolerance)
    LaunchedEffect(scrollState.value, scrollState.maxValue) {
        if (scrollState.maxValue > 0 && scrollState.value >= scrollState.maxValue - 50) {
            hasScrolledToBottom = true
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(ZColors.terminalBlack),
    ) {
        // Scrollable content
        Column(
            modifier = Modifier
                .weight(1f)
                .verticalScroll(scrollState)
                .padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            // Header
            Image(
                painter = painterResource(id = R.drawable.zipherx_logo),
                contentDescription = "ZipherX logo",
                modifier = Modifier.size(56.dp),
            )
            Spacer(Modifier.height(12.dp))
            Text(
                "ZIPHERX",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                fontSize = 26.sp,
                color = ZColors.primary,
                letterSpacing = 4.sp,
            )
            Spacer(Modifier.height(4.dp))
            Text(
                "IMPORTANT LEGAL NOTICE",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.SemiBold,
                fontSize = 13.sp,
                color = ZColors.primary.copy(alpha = 0.8f),
            )
            Spacer(Modifier.height(20.dp))

            // Section 1
            DisclaimerSection(
                title = "1. OPEN SOURCE SOFTWARE",
                content = "ZipherX is free, open-source software distributed under the MIT License. " +
                    "This application is a tool that enables users to interact with the Zclassic blockchain network. " +
                    "The software is provided \"as is\" without any representations or warranties of any kind, " +
                    "either express or implied.",
            )

            // Section 2
            DisclaimerSection(
                title = "2. PRIVACY AS A FUNDAMENTAL RIGHT",
                content = "Privacy is a fundamental human right recognized by the United Nations Declaration of Human Rights, " +
                    "the International Covenant on Civil and Political Rights, and numerous other international and regional treaties. " +
                    "ZipherX implements cryptographic privacy features that exist to protect this fundamental right. " +
                    "Financial privacy is essential for personal security, protection from discrimination, " +
                    "and the preservation of human dignity.",
            )

            // Section 3
            DisclaimerSection(
                title = "3. NON-CUSTODIAL ARCHITECTURE",
                content = "ZipherX is a non-custodial wallet. The developer(s) of this software:\n\n" +
                    "\u2022 Have NO access to your private keys or funds\n" +
                    "\u2022 Cannot freeze, seize, or control your assets\n" +
                    "\u2022 Cannot reverse, cancel, or modify any transactions\n" +
                    "\u2022 Do NOT collect, store, or transmit any personal data\n" +
                    "\u2022 Do NOT operate any central servers or maintain any logs\n\n" +
                    "Your keys are stored exclusively on your device using hardware-backed encryption.",
            )

            // Section 4
            DisclaimerSection(
                title = "4. DECENTRALIZED NETWORK",
                content = "ZipherX connects directly to the peer-to-peer Zclassic network. " +
                    "There is no central server, no intermediary, and no single point of control. " +
                    "The software is merely an interface to interact with a decentralized, " +
                    "permissionless blockchain network that operates independently of any individual or organization.",
            )

            // Section 5
            DisclaimerSection(
                title = "5. USER RESPONSIBILITY",
                content = "By using this software, you acknowledge and agree that:\n\n" +
                    "\u2022 YOU are solely responsible for compliance with all applicable laws and regulations in your jurisdiction\n" +
                    "\u2022 YOU are responsible for securing your recovery phrase and private keys\n" +
                    "\u2022 YOU are responsible for verifying transaction details before confirmation\n" +
                    "\u2022 YOU understand that blockchain transactions are irreversible\n" +
                    "\u2022 YOU accept all risks associated with using cryptocurrency software",
            )

            // Section 6
            DisclaimerSection(
                title = "6. NO FINANCIAL ADVICE",
                content = "Nothing in this software constitutes financial, investment, legal, or tax advice. " +
                    "The developer(s) are not financial advisors. " +
                    "You should consult qualified professionals for any financial decisions. " +
                    "Cryptocurrency values are volatile and you may lose some or all of your investment.",
            )

            // Section 7
            DisclaimerSection(
                title = "7. LIMITATION OF LIABILITY",
                content = "TO THE MAXIMUM EXTENT PERMITTED BY APPLICABLE LAW, IN NO EVENT SHALL THE DEVELOPERS, " +
                    "CONTRIBUTORS, OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES, OR OTHER LIABILITY, " +
                    "WHETHER IN AN ACTION OF CONTRACT, TORT, OR OTHERWISE, ARISING FROM, OUT OF, " +
                    "OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.\n\n" +
                    "This includes but is not limited to: loss of funds, loss of profits, loss of data, " +
                    "business interruption, or any indirect, incidental, special, or consequential damages.",
            )

            // Section 8
            DisclaimerSection(
                title = "8. INTENDED USE",
                content = "This software is intended for legitimate privacy-preserving financial transactions. " +
                    "Legitimate uses include but are not limited to:\n\n" +
                    "\u2022 Protecting personal financial information from data breaches\n" +
                    "\u2022 Preventing financial surveillance and profiling\n" +
                    "\u2022 Protecting business confidentiality\n" +
                    "\u2022 Donations to sensitive causes (journalism, activism, charity)\n" +
                    "\u2022 Personal security in high-risk environments\n\n" +
                    "The existence of privacy tools does not imply endorsement of any illegal activity.",
            )

            // Section 9
            DisclaimerSection(
                title = "9. JURISDICTIONAL NOTICE",
                content = "Cryptocurrency regulations vary by jurisdiction. Some features of this software may not be " +
                    "legal in all jurisdictions. It is YOUR responsibility to ensure that your use of this " +
                    "software complies with all applicable laws in your location. " +
                    "The developer(s) make no representations regarding the legality of this software in any jurisdiction.",
            )

            // Section 10
            DisclaimerSection(
                title = "10. EXPERIMENTAL SOFTWARE",
                content = "ZipherX is beta software under active development. It may contain bugs, errors, " +
                    "defects, or incomplete features that could result in loss of funds, corrupted data, " +
                    "or unexpected behavior. There is NO guarantee that the software will function correctly, " +
                    "continuously, or without interruption. " +
                    "DO NOT use this software with funds you cannot afford to lose entirely and permanently.",
            )

            // Section 11
            DisclaimerSection(
                title = "11. INDEMNIFICATION",
                content = "BY USING THIS SOFTWARE, YOU AGREE TO INDEMNIFY, DEFEND, AND HOLD HARMLESS THE DEVELOPERS, " +
                    "CONTRIBUTORS, AND COPYRIGHT HOLDERS FROM AND AGAINST ANY AND ALL CLAIMS, LIABILITIES, " +
                    "DAMAGES, LOSSES, COSTS, AND EXPENSES (INCLUDING REASONABLE LEGAL FEES) ARISING OUT OF OR " +
                    "RELATED TO YOUR USE OR MISUSE OF THIS SOFTWARE, YOUR VIOLATION OF THIS DISCLAIMER, " +
                    "OR YOUR VIOLATION OF ANY APPLICABLE LAW OR REGULATION.",
            )

            // Section 12
            DisclaimerSection(
                title = "12. THIRD-PARTY SERVICES & FORCE MAJEURE",
                content = "ZipherX relies on third-party decentralized services including but not limited to: " +
                    "the Zclassic blockchain network, the Tor anonymity network, and peer-to-peer node operators. " +
                    "The developer(s) have NO control over these networks and accept NO responsibility for:\n\n" +
                    "\u2022 Network outages, congestion, or failures\n" +
                    "\u2022 Blockchain forks, reorganizations, or protocol changes\n" +
                    "\u2022 Tor network disruptions or de-anonymization attacks\n" +
                    "\u2022 Malicious peer nodes or Sybil attacks\n" +
                    "\u2022 Acts of God, war, government action, or any event beyond reasonable control\n\n" +
                    "Your use of these third-party networks is entirely at your own risk.",
            )

            // Section 13
            DisclaimerSection(
                title = "13. BACKUP WARNING",
                content = "YOU MUST BACK UP YOUR WALLET BEFORE INSTALLING OR USING ZIPHERX. " +
                    "If you are running an existing Zclassic full node or any other wallet software, " +
                    "back up ALL wallet files, private keys, and spending keys BEFORE proceeding. " +
                    "The developer(s) accept NO responsibility for loss of funds or data " +
                    "resulting from failure to maintain adequate backups. " +
                    "ALWAYS maintain independent, offline backups of your keys and wallet files. " +
                    "Never rely solely on any single piece of software to protect your funds.",
            )

            // Section 14
            DisclaimerSection(
                title = "14. VOLUNTARY CONTRIBUTIONS",
                content = "All contributions to the development of ZipherX — including but not limited to code, " +
                    "documentation, design, testing, bug reports, translations, and feedback — are made " +
                    "on a strictly voluntary and unpaid basis. Contributing to this project does NOT entitle " +
                    "any contributor to any form of compensation, ownership, equity, or decision-making authority.",
            )

            Spacer(Modifier.height(12.dp))

            // Cypherpunk quote
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(ZColors.primary.copy(alpha = 0.08f), RoundedCornerShape(2.dp))
                    .padding(12.dp),
            ) {
                Text(
                    "\"Privacy is necessary for an open society in the electronic age. Privacy is not secrecy. " +
                        "A private matter is something one doesn't want the whole world to know, but a secret matter " +
                        "is something one doesn't want anybody to know. Privacy is the power to selectively reveal oneself to the world.\"",
                    fontFamily = FontFamily.Monospace,
                    fontSize = 10.sp,
                    color = ZColors.primary.copy(alpha = 0.9f),
                    lineHeight = 16.sp,
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    "- Eric Hughes, A Cypherpunk's Manifesto (1993)",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 9.sp,
                    color = ZColors.primary.copy(alpha = 0.7f),
                )
            }

            Spacer(Modifier.height(12.dp))

            // Acknowledgment
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, Color.White.copy(alpha = 0.2f), RoundedCornerShape(2.dp))
                    .background(Color.White.copy(alpha = 0.05f), RoundedCornerShape(2.dp))
                    .padding(12.dp),
            ) {
                Text(
                    "BY PROCEEDING, YOU ACKNOWLEDGE THAT:",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 10.sp,
                    color = Color.White,
                )
                Spacer(Modifier.height(8.dp))
                AckItem("You have read and understood all 14 sections of this disclaimer")
                AckItem("You are at least 18 years of age or the age of majority in your jurisdiction")
                AckItem("You accept full responsibility for your use of this software")
                AckItem("You will comply with all applicable laws in your jurisdiction")
                AckItem("You understand the risks of using cryptocurrency and beta software")
                AckItem("You agree to the indemnification terms in Section 11")
                AckItem("You have backed up all existing wallet files and keys before using this software")
            }

            Spacer(Modifier.height(16.dp))
        }

        // Fixed accept button at bottom
        HorizontalDivider(color = ZColors.primary.copy(alpha = 0.3f))

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .background(ZColors.terminalBlack)
                .padding(horizontal = 20.dp, vertical = 12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            if (!hasScrolledToBottom) {
                Text(
                    "Scroll down to read the entire disclaimer...",
                    fontFamily = FontFamily.Monospace,
                    fontSize = 10.sp,
                    color = ZColors.primary.copy(alpha = 0.6f),
                )
                Spacer(Modifier.height(8.dp))
            }

            OutlinedButton(
                onClick = onAccept,
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(2.dp),
                border = BorderStroke(
                    1.dp,
                    if (hasScrolledToBottom) ZColors.primary else ZColors.primary.copy(alpha = 0.3f),
                ),
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = if (hasScrolledToBottom) ZColors.primary else ZColors.primaryDim,
                ),
                enabled = hasScrolledToBottom,
            ) {
                Text(
                    "I ACCEPT AND UNDERSTAND",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(vertical = 4.dp),
                )
            }
        }
    }
}

@Composable
private fun DisclaimerSection(title: String, content: String) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 10.dp)
            .border(1.dp, ZColors.primary.copy(alpha = 0.2f), RoundedCornerShape(2.dp))
            .background(ZColors.primary.copy(alpha = 0.05f), RoundedCornerShape(2.dp))
            .padding(12.dp),
    ) {
        Text(
            title,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            fontSize = 11.sp,
            color = ZColors.primary,
        )
        Spacer(Modifier.height(6.dp))
        Text(
            content,
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            color = Color.White.copy(alpha = 0.85f),
            lineHeight = 16.sp,
        )
    }
}

@Composable
private fun AckItem(text: String) {
    Row(
        modifier = Modifier.padding(vertical = 2.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Text(
            "> ",
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            fontSize = 10.sp,
            color = ZColors.primary,
        )
        Text(
            text,
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            color = Color.White.copy(alpha = 0.8f),
        )
    }
}
