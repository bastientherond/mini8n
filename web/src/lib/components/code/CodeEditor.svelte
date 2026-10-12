<script lang="ts" module>
	export type EditorMode = 'expression' | 'template' | 'sql' | 'json';

	export interface EditorApi {
		/** Insère une référence (`$json.prix`) au curseur, entre `{{ }}` si nécessaire. */
		insertRef: (ref: string) => void;
		/** Insère du texte brut au curseur. */
		insert: (text: string) => void;
		focus: () => void;
	}

	/** Type MIME des colonnes glissées depuis les tableaux de données. */
	export const FIELD_MIME = 'application/x-mini8n-field';
</script>

<script lang="ts">
	// Éditeur de code (CodeMirror 6) pour les expressions, les textes `{{ … }}`, le SQL et le JSON.
	import {
		autocompletion,
		closeBrackets,
		closeBracketsKeymap,
		completionKeymap,
		completionStatus,
		startCompletion,
	} from '@codemirror/autocomplete';
	import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
	import { json } from '@codemirror/lang-json';
	import { PostgreSQL, sql } from '@codemirror/lang-sql';
	import { bracketMatching } from '@codemirror/language';
	import { type Diagnostic, setDiagnostics } from '@codemirror/lint';
	import { EditorState, type Extension, Prec } from '@codemirror/state';
	import {
		Decoration,
		EditorView,
		MatchDecorator,
		ViewPlugin,
		type ViewUpdate,
		keymap,
		placeholder as placeholderExt,
	} from '@codemirror/view';
	import { onMount } from 'svelte';
	import { completionSource, expressionBefore } from '$lib/expressions/completion';
	import { type ExprContext, emptyContext } from '$lib/expressions/context';
	import { expressionLanguage, highlight, templateLanguage } from '$lib/expressions/language';
	import { cn } from '$lib/utils';

	let {
		value,
		onchange,
		mode,
		singleLine = false,
		placeholder = '',
		context = emptyContext,
		diagnostics = [],
		id,
		class: className = '',
		ariaLabel,
		autofocus = false,
		onready,
		onfocuschange,
	}: {
		value: string;
		onchange: (value: string) => void;
		mode: EditorMode;
		singleLine?: boolean;
		placeholder?: string;
		context?: ExprContext;
		diagnostics?: Diagnostic[];
		id?: string;
		class?: string;
		ariaLabel?: string;
		autofocus?: boolean;
		onready?: (api: EditorApi) => void;
		onfocuschange?: (focused: boolean) => void;
	} = $props();

	let host: HTMLDivElement;
	let view: EditorView | undefined;
	let dropping = $state(false);
	/** Mise à jour venue de l'extérieur : ne pas la renvoyer à `onchange`. */
	let syncing = false;
	// Lu par l'autocomplétion à chaque frappe, sans reconfigurer l'éditeur.
	let currentContext: ExprContext = emptyContext;
	$effect(() => {
		currentContext = context;
	});

	const blockMarks = new MatchDecorator({
		regexp: /\{\{[^]*?\}\}/g,
		decoration: Decoration.mark({ class: 'cm-expr-block' }),
	});
	const blockPlugin = ViewPlugin.define(
		(v) => ({
			decorations: blockMarks.createDeco(v),
			update(u: ViewUpdate) {
				this.decorations = blockMarks.updateDeco(u, this.decorations);
			},
		}),
		{ decorations: (p) => p.decorations },
	);

	/** Saisie de `{{` : ouvre un bloc `{{ | }}` et affiche les suggestions. */
	const braceHandler = EditorView.inputHandler.of((v, from, to, text) => {
		if (!text.endsWith('{')) return false;
		const before = v.state.sliceDoc(Math.max(0, from - 1), from) + text;
		if (!before.endsWith('{{') || v.state.sliceDoc(to, to + 2) === '}}') return false;
		v.dispatch({ changes: { from, to, insert: `${text}  }}` }, selection: { anchor: from + text.length + 1 } });
		startCompletion(v);
		return true;
	});

	/** Ligne unique : les retours à la ligne collés deviennent des espaces. */
	const oneLine = [
		EditorState.transactionFilter.of((tr) => {
			if (!tr.docChanged || tr.newDoc.lines === 1) return tr;
			const text = tr.newDoc.toString().replace(/\r?\n/g, ' ');
			return [tr, { changes: { from: 0, to: tr.newDoc.length, insert: text }, sequential: true }];
		}),
		Prec.low(keymap.of([{ key: 'Enter', run: () => true }])),
	];

	function insertRef(ref: string) {
		if (!view) return;
		const { from, to } = view.state.selection.main;
		const before = view.state.sliceDoc(0, from);
		const inBlock = expressionBefore(before, true) !== null;
		const text = mode === 'template' && !inBlock ? `{{ ${ref} }}` : ref;
		view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length }, scrollIntoView: true });
		view.focus();
	}

	function insert(text: string) {
		if (!view) return;
		const { from, to } = view.state.selection.main;
		view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length } });
		view.focus();
	}

	function extensions(): Extension[] {
		const ext: Extension[] = [
			history(),
			keymap.of([...closeBracketsKeymap, ...completionKeymap, ...defaultKeymap, ...historyKeymap]),
			EditorView.lineWrapping,
			highlight,
			bracketMatching(),
			EditorView.contentAttributes.of({
				spellcheck: 'false',
				autocorrect: 'off',
				autocapitalize: 'off',
				...(ariaLabel ? { 'aria-label': ariaLabel } : {}),
				...(id ? { id } : {}),
			}),
			EditorView.updateListener.of((u) => {
				if (u.docChanged && !syncing) onchange(u.state.doc.toString());
				if (u.focusChanged) onfocuschange?.(u.view.hasFocus);
			}),
			EditorView.domEventHandlers({
				dragover(event) {
					if (!event.dataTransfer?.types.includes(FIELD_MIME)) return false;
					event.preventDefault();
					dropping = true;
					return true;
				},
				dragleave() {
					dropping = false;
					return false;
				},
				drop(event, v) {
					dropping = false;
					const ref = event.dataTransfer?.getData(FIELD_MIME);
					if (!ref) return false;
					event.preventDefault();
					const pos = v.posAtCoords({ x: event.clientX, y: event.clientY }) ?? v.state.doc.length;
					v.dispatch({ selection: { anchor: pos } });
					insertRef(ref);
					return true;
				},
			}),
		];
		if (placeholder) ext.push(placeholderExt(placeholder));
		if (singleLine) ext.push(oneLine);
		if (mode === 'expression' || mode === 'template') {
			ext.push(
				mode === 'template' ? templateLanguage : expressionLanguage,
				closeBrackets(),
				autocompletion({
					override: [completionSource(() => currentContext, mode === 'template')],
					icons: true,
					maxRenderedOptions: 100,
					tooltipClass: () => 'cm-mini8n-tooltip',
				}),
			);
			if (mode === 'template') ext.push(blockPlugin, braceHandler);
		} else if (mode === 'sql') {
			ext.push(sql({ dialect: PostgreSQL, upperCaseKeywords: true }), closeBrackets(), autocompletion({ icons: true }));
		} else {
			ext.push(json(), closeBrackets());
		}
		return ext;
	}

	onMount(() => {
		view = new EditorView({
			parent: host,
			state: EditorState.create({ doc: value, extensions: extensions() }),
		});
		onready?.({ insertRef, insert, focus: () => view?.focus() });
		// Échap ferme d'abord les suggestions, sans fermer la fenêtre qui contient l'éditeur.
		let swallowEscape = false;
		const capture = (e: KeyboardEvent) => {
			swallowEscape = e.key === 'Escape' && !!view && completionStatus(view.state) !== null;
		};
		const bubble = (e: KeyboardEvent) => {
			if (e.key === 'Escape' && swallowEscape) e.stopPropagation();
			swallowEscape = false;
		};
		host.addEventListener('keydown', capture, true);
		host.addEventListener('keydown', bubble);
		if (autofocus) {
			view.focus();
			view.dispatch({ selection: { anchor: view.state.doc.length } });
		}
		return () => view?.destroy();
	});

	// Valeur modifiée de l'extérieur (annulation, autre nœud…).
	$effect(() => {
		const v = value;
		if (view && v !== view.state.doc.toString()) {
			syncing = true;
			view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: v } });
			syncing = false;
		}
	});

	$effect(() => {
		const d = diagnostics;
		if (!view) return;
		const len = view.state.doc.length;
		const clamped = d
			.map((x) => ({ ...x, from: Math.min(x.from, len), to: Math.min(Math.max(x.to, x.from), len) }))
			.filter((x) => x.to > x.from || len === 0);
		view.dispatch(setDiagnostics(view.state, clamped));
	});
</script>

<div
	bind:this={host}
	class={cn(
		'mini8n-code border-input dark:bg-input/30 relative w-full min-w-0 rounded-lg border bg-transparent text-xs transition-[color,box-shadow]',
		'focus-within:border-ring focus-within:ring-ring/50 focus-within:ring-3',
		singleLine ? 'mini8n-code-single' : 'mini8n-code-multi',
		dropping && 'border-primary ring-primary/30 ring-3',
		className,
	)}
></div>
