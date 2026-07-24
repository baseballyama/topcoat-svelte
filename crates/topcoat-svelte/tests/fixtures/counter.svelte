<script>
	let { count = 0 } = $props();
	let n = $state(count);
</script>

<button onclick={() => (n += 1)}>count {n}</button>
