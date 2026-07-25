<script>
	let { title = "Untitled", count = 0 } = $props();
</script>

<svelte:head>
	<title>{title}</title>
</svelte:head>

<main>
	<h1>{title}</h1>
	<p>count {count}</p>
</main>
