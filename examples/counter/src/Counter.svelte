<script>
	let { count = 0 } = $props();
	let n = $state(count);
</script>

<button onclick={() => (n += 1)}>
	clicked {n} {n === 1 ? "time" : "times"}
</button>

<style>
	button {
		font: inherit;
		padding: 0.5rem 1rem;
		border-radius: 0.375rem;
		border: 1px solid currentColor;
		cursor: pointer;
	}
</style>
