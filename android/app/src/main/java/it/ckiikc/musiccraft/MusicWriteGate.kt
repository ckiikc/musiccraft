package it.ckiikc.musiccraft

/** Shared only by background workers; never wait for this lock on the main thread. */
object MusicWriteGate { val lock=java.util.concurrent.locks.ReentrantLock() }
