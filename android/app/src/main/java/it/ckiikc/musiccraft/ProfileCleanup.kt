package it.ckiikc.musiccraft

/** Releasing a Bluetooth proxy may synchronously disconnect other proxies. */
internal fun <T> closeProfileProxies(proxies:MutableMap<Int,T>, release:(Int,T)->Unit) {
    val detached=proxies.toMap()
    proxies.clear()
    detached.forEach { (id,proxy) -> release(id,proxy) }
}
