package com.cosct.pdfcompressor

import android.content.Context
import android.net.Uri
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.*
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.Assert.*
import org.mockito.Mockito.mock
import uniffi.pdfcompressor.*

@OptIn(ExperimentalCoroutinesApi::class)
class CompressViewModelTest {
    private val dispatcher = StandardTestDispatcher()
    private val context = mock(Context::class.java)
    private val input = Stage.Analyzing(mock(Uri::class.java), byteArrayOf(1), "locked.pdf")
    private val analysis = FfiAnalysis(1uL, 1u, 0u, "mixed", 0f, 0f, 0f, "balanced", 100u, 100u, 70u, false, emptyList())
    private var lastPassword: String? = null
    private val engine = object : PdfEngineApi {
        override fun probe() = "test"
        override fun presetTable() = emptyMap<String, FfiPresetProfile>()
        override suspend fun analyze(bytes: ByteArray, password: String?, progress: PdfEngine.Progress): FfiAnalysis {
            lastPassword = password
            if (password == null) throw FfiException.PasswordRequired()
            if (password != "correct") throw FfiException.WrongPassword()
            return analysis
        }
        override suspend fun compress(bytes: ByteArray, password: String?, settings: FfiSettings, progress: PdfEngine.Progress): FfiCompressResult = awaitCancellation()
        override suspend fun compressToTarget(bytes: ByteArray, targetBytes: ULong, password: String?, settings: FfiSettings, progress: PdfEngine.Progress): FfiCompressResult = awaitCancellation()
    }
    @Before fun setUp() { Dispatchers.setMain(dispatcher) }
    @After fun tearDown() { Dispatchers.resetMain() }

    @Test fun passwordRetryStartsAtAnalysisAndRetainsInput() = runTest(dispatcher) {
        val vm = CompressViewModel(engine)
        vm.retryAnalysis(context, input, null)
        runCurrent()
        assertEquals(input, vm.pendingPassword?.first)
        assertEquals(false, vm.pendingPassword?.second)
        vm.retryAnalysis(context, input, "wrong")
        runCurrent()
        assertEquals(true, vm.pendingPassword?.second)
        vm.retryAnalysis(context, input, "correct")
        runCurrent()
        assertTrue(vm.stage is Stage.Ready)
        assertNull(vm.pendingPassword)
        assertEquals("correct", lastPassword)
        vm.resetToIdle()
    }

    @Test fun coroutineCancellationReturnsToReady() = runTest(dispatcher) {
        val vm = CompressViewModel(engine)
        val ready = Stage.Ready(input.uri, input.bytes, input.displayName, analysis, null)
        vm.runCompress(context, ready, null, "balanced")
        runCurrent()
        assertTrue(vm.stage is Stage.Compressing)
        vm.cancelCompress()
        runCurrent()
        assertEquals(ready, vm.stage)
        vm.resetToIdle()
    }

    @Test fun oldCancellationCannotOverwriteNewAnalysis() = runTest(dispatcher) {
        val vm = CompressViewModel(engine)
        val ready = Stage.Ready(input.uri, input.bytes, input.displayName, analysis, null)
        vm.runCompress(context, ready, null, "balanced")
        runCurrent()
        vm.retryAnalysis(context, input, "correct")
        runCurrent()
        assertEquals("correct", (vm.stage as Stage.Ready).password)
        vm.resetToIdle()
    }
}
