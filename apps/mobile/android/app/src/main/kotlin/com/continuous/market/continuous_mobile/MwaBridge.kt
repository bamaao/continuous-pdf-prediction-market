package com.continuous.market.continuous_mobile

import android.net.Uri
import android.util.Base64
import androidx.lifecycle.lifecycleScope
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import com.solana.mobilewalletadapter.clientlib.ConnectionIdentity
import com.solana.mobilewalletadapter.clientlib.MobileWalletAdapter
import com.solana.mobilewalletadapter.clientlib.Solana
import com.solana.mobilewalletadapter.clientlib.TransactionResult
import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import kotlinx.coroutines.launch

/**
 * Local Mobile Wallet Adapter session. Signs only. The web desk submits the
 * signed bytes to its own RPC, so a wallet cluster mismatch cannot redirect settlement.
 */
class MwaBridge(private val activity: FlutterFragmentActivity) : MethodChannel.MethodCallHandler {
    private var adapter: MobileWalletAdapter? = null
    private var identityUri: Uri? = null

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        activity.lifecycleScope.launch {
            try {
                when (call.method) {
                    "authorize" -> result.success(authorize(call.argument("cluster"), call.argument("origin")))
                    "signTransactions" -> result.success(signTransactions(call.argument("transactions")))
                    "signMessage" -> result.success(signMessage(call.argument("message")))
                    "disconnect" -> {
                        disconnect()
                        result.success(null)
                    }
                    else -> result.notImplemented()
                }
            } catch (e: Exception) {
                result.error("MWA", e.message ?: "wallet request failed", null)
            }
        }
    }

    private fun session(origin: String?): MobileWalletAdapter {
        val uri = origin?.takeIf { it.startsWith("https://") }?.let(Uri::parse)
            ?: Uri.parse("https://continuous.market")
        val current = adapter
        if (current != null && identityUri == uri) return current
        val created = MobileWalletAdapter(
            ConnectionIdentity(
                identityUri = uri,
                iconUri = Uri.parse("https://continuous.market/favicon.ico"),
                identityName = "Continuous",
            ),
        )
        adapter = created
        identityUri = uri
        return created
    }

    private fun applyCluster(wallet: MobileWalletAdapter, cluster: String?) {
        wallet.blockchain = when (cluster) {
            "mainnet-beta", "mainnet" -> Solana.Mainnet
            "testnet" -> Solana.Testnet
            else -> Solana.Devnet
        }
    }

    private suspend fun authorize(cluster: String?, origin: String?): Map<String, String> {
        val wallet = session(origin)
        applyCluster(wallet, cluster)
        return when (val outcome = wallet.transact(ActivityResultSender(activity)) { auth ->
            val account = auth.accounts.firstOrNull()
                ?: throw IllegalStateException("wallet returned no account")
            b64(account.publicKey)
        }) {
            is TransactionResult.Success -> mapOf("publicKey" to (outcome.payload as String))
            is TransactionResult.NoWalletFound -> throw IllegalStateException("No Mobile Wallet Adapter app is installed")
            is TransactionResult.Failure -> throw IllegalStateException(outcome.message)
        }
    }

    private suspend fun signTransactions(encoded: List<String>?): Map<String, List<String>> {
        val payloads = encoded?.map { b64Decode(it) }?.toTypedArray()
            ?: throw IllegalArgumentException("transactions required")
        val wallet = adapter ?: throw IllegalStateException("connect the mobile wallet first")
        return when (val outcome = wallet.transact(ActivityResultSender(activity)) {
            @Suppress("DEPRECATION")
            signTransactions(payloads).signedPayloads.map { b64(it) }
        }) {
            is TransactionResult.Success -> {
                @Suppress("UNCHECKED_CAST")
                mapOf("transactions" to (outcome.payload as List<String>))
            }
            is TransactionResult.NoWalletFound -> throw IllegalStateException("No Mobile Wallet Adapter app is installed")
            is TransactionResult.Failure -> throw IllegalStateException(outcome.message)
        }
    }

    private suspend fun signMessage(encoded: String?): Map<String, String> {
        val message = encoded?.let { b64Decode(it) } ?: throw IllegalArgumentException("message required")
        val wallet = adapter ?: throw IllegalStateException("connect the mobile wallet first")
        return when (val outcome = wallet.transact(ActivityResultSender(activity)) { auth ->
            val account = auth.accounts.firstOrNull()
                ?: throw IllegalStateException("wallet returned no account")
            signMessagesDetached(arrayOf(message), arrayOf(account.publicKey))
                .messages
                .first()
                .signatures
                .first()
                .let { b64(it) }
        }) {
            is TransactionResult.Success -> mapOf("signature" to (outcome.payload as String))
            is TransactionResult.NoWalletFound -> throw IllegalStateException("No Mobile Wallet Adapter app is installed")
            is TransactionResult.Failure -> throw IllegalStateException(outcome.message)
        }
    }

    private suspend fun disconnect() {
        val wallet = adapter ?: return
        wallet.disconnect(ActivityResultSender(activity))
        adapter = null
        identityUri = null
    }

    private fun b64(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.NO_WRAP)

    private fun b64Decode(value: String): ByteArray = Base64.decode(value, Base64.NO_WRAP)
}
