<script>
	import Counter from './Counter.svelte';
	import Label from './Label.svelte';
	let { count = 0 } = $props();
</script>

<svelte:head>
	<title>Rust + Svelte page</title>
</svelte:head>

<main>
	<h1>Full Svelte page</h1>
	<p>This whole document is a single Svelte component tree, rendered by Rust.</p>
	<p><Label text="Live counter:" /> the button below is the same component reused.</p>
	<Counter {count} />
	<nav>
		<!-- Soft navigation between the two Svelte pages (no full reload). -->
		<a href="/about">About page</a>
		<!-- The island page is not a Svelte page, so this full-navigates. -->
		<a href="/">Island demo</a>
	</nav>
</main>

<style>
	main {
		font-family: system-ui, sans-serif;
		max-width: 40rem;
		margin: 2rem auto;
	}
</style>
