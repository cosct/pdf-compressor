package com.cosct.pdfcompressor

import java.io.ByteArrayInputStream
import java.io.InputStream
import org.junit.Assert.*
import org.junit.Test
import uniffi.pdfcompressor.FfiException

class PdfIoTest {
    @Test fun exactLimitAndEmptyStreamSucceed() {
        assertArrayEquals(byteArrayOf(1, 2, 3), readLimited(ByteArrayInputStream(byteArrayOf(1, 2, 3)), 3))
        assertArrayEquals(byteArrayOf(), readLimited(ByteArrayInputStream(byteArrayOf()), 3))
    }

    @Test fun unknownLengthStreamStopsAfterLimitPlusOne() {
        var read = 0
        val endless = object : InputStream() {
            override fun read(): Int { read += 1; return 1 }
        }
        try { readLimited(endless, 17); fail("must reject") }
        catch (error: FfiException.InputTooLarge) { assertEquals(17uL, error.limitBytes) }
        assertEquals(18, read)
    }

    @Test fun cancelledReadDoesNotConsumeInput() {
        val stream = ByteArrayInputStream(byteArrayOf(1, 2, 3))
        try { readLimited(stream, 3) { throw kotlinx.coroutines.CancellationException() }; fail("must cancel") }
        catch (_: kotlinx.coroutines.CancellationException) { assertEquals(3, stream.available()) }
    }

    @Test fun failedWriteDeletesOnlyItsOwnedOutput() {
        var deleted = false
        val broken = object : java.io.OutputStream() {
            override fun write(value: Int) { throw java.io.IOException("disk full") }
        }
        try { writeOwnedOutput(byteArrayOf(1), { broken }, { deleted = true }); fail("must fail") }
        catch (_: java.io.IOException) { assertTrue(deleted) }
    }

    @Test fun nullOutputStreamIsAnErrorAndCleansUp() {
        var deleted = false
        try { writeOwnedOutput(byteArrayOf(1), { null }, { deleted = true }); fail("must fail") }
        catch (_: java.io.IOException) { assertTrue(deleted) }
    }

    @Test fun completedWriteKeepsOutput() {
        val output = java.io.ByteArrayOutputStream()
        writeOwnedOutput(byteArrayOf(1, 2), { output }, { fail("must keep output") })
        assertArrayEquals(byteArrayOf(1, 2), output.toByteArray())
    }
}
