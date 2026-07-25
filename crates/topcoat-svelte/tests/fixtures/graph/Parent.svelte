<script>
	import Child from './Child.svelte';
	let { count = 0 } = $props();
	// A string that looks like an import specifier but is only data; the macro
	// must not rewrite it.
	let note = "loaded from ./fake.svelte (not a real import)";
</script>

<section>
	<h1>{note}</h1>
	<Child name={`item ${count}`} />
</section>
