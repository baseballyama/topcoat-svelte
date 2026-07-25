<script>
	import Grandchild from './Grandchild.svelte';
	let { name = "child" } = $props();
</script>

<div class="child">
	<p>{name}</p>
	<Grandchild label={name} />
</div>
