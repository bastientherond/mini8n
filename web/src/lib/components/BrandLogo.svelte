<script lang="ts">
	// Logo d'un service sur sa tuile de couleur (voir $lib/brands.ts : les logos sont
	// définis dans le code et ne se configurent pas).
	import { type BrandKey, brands } from '$lib/brands';
	import { cn } from '$lib/utils';

	let { brand, class: className = '' }: { brand: BrandKey; class?: string } = $props();

	const b = $derived(brands[brand]);
</script>

<span
	class={cn('flex shrink-0 items-center justify-center overflow-hidden rounded-lg border border-black/10 dark:border-white/10', className)}
	style="background: {b.bg}; color: {b.fg};"
	role="img"
	aria-label={b.label}
	title={b.label}
>
	<!-- eslint-disable-next-line svelte/no-at-html-tags : constante du code, pas une donnée utilisateur -->
	<svg viewBox="0 0 24 24" class="size-[70%]" aria-hidden="true">{@html b.svg}</svg>
</span>
