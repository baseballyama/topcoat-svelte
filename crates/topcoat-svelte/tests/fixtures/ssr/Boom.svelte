<script>
	let { boom = false } = $props();
	// Throws at render time when asked to, to exercise the SSR error fallback.
	if (boom) {
		throw new Error("kaboom");
	}
</script>

<p>safe</p>
