package com.cosct.pdfcompressor

import androidx.work.*
import org.junit.Test
import org.mockito.Mockito.*

class QueueChainTest {
    @Test fun eachItemDependsOnThePreviousItem() {
        val manager = mock(WorkManager::class.java)
        val first = mock(OneTimeWorkRequest::class.java)
        val second = mock(OneTimeWorkRequest::class.java)
        val third = mock(OneTimeWorkRequest::class.java)
        val a = mock(WorkContinuation::class.java)
        val b = mock(WorkContinuation::class.java)
        val c = mock(WorkContinuation::class.java)
        `when`(manager.beginUniqueWork(QueueWorker.QUEUE_CHAIN, ExistingWorkPolicy.APPEND_OR_REPLACE, first)).thenReturn(a)
        `when`(a.then(second)).thenReturn(b)
        `when`(b.then(third)).thenReturn(c)
        enqueueSequential(manager, listOf(first, second, third))
        verify(a).then(second)
        verify(b).then(third)
        verify(c).enqueue()
        verify(a, never()).enqueue()
        verify(b, never()).enqueue()
    }
    @Test fun emptyBatchDoesNotCreateWork() {
        val manager = mock(WorkManager::class.java)
        enqueueSequential(manager, emptyList())
        verifyNoInteractions(manager)
    }
}
