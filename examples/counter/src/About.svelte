<script>
	import Label from './Label.svelte';
	let { message = "" } = $props();
</script>

<svelte:head>
	<title>About — Rust + Svelte</title>
</svelte:head>

<main>
	<h1>About</h1>
	<p><Label text="Message:" /> {message}</p>
	<nav>
		<!-- Soft navigation back to the other Svelte page. -->
		<a href="/page">Home page</a>
		<!-- Full navigation to the island demo. -->
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
