<script lang="ts">
	// Logo mini8n : trois nœuds reliés, sur le dégradé de la marque.
	import { cn } from '$lib/utils';

	let { class: className = '' }: { class?: string } = $props();
</script>

<span class={cn('bg-brand flex shrink-0 items-center justify-center rounded-lg text-white shadow-sm', className)}>
	<svg viewBox="0 0 24 24" class="size-[60%]" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true">
		<path d="M7 7.5 10.5 11M7 16.5l3.5-3.5M13.5 12H17" />
		<circle cx="5.5" cy="6" r="2.3" fill="currentColor" stroke="none" />
		<circle cx="5.5" cy="18" r="2.3" fill="currentColor" stroke="none" />
		<circle cx="12" cy="12" r="2.6" fill="currentColor" stroke="none" />
		<circle cx="19" cy="12" r="2.3" fill="currentColor" stroke="none" opacity="0.75" />
	</svg>
</span>
