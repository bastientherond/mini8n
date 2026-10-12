// Autocomplétion contextuelle des expressions : colonnes de `$json`, sorties des
// autres nœuds, méthodes selon le type de la valeur (texte, nombre, date…).

import type { Completion, CompletionContext, CompletionResult, CompletionSection } from '@codemirror/autocomplete';
import type { EditorView } from '@codemirror/view';
import { engine } from '$lib/engine/client';
import type { ColumnInfo, ExprDoc } from '$lib/engine/types';
import { type ExprContext, type Table, kindLabels, kindOf, sampleRow, shortValue } from './context';

const IDENT = /^[\p{L}_$][\p{L}\p{N}_$]*$/u;
const VALUE_KINDS = ['string', 'number', 'list', 'object', 'date', 'boolean'];

const sections: Record<string, CompletionSection> = {
	columns: { name: 'Colonnes', rank: 0 },
	vars: { name: 'Variables', rank: 1 },
	nodes: { name: 'Autres nœuds', rank: 2 },
	methods: { name: 'Méthodes', rank: 3 },
	functions: { name: 'Fonctions', rank: 4 },
	legacy: { name: 'Anciennes fonctions', rank: 5 },
};

/** Type déduit d'une partie d'expression. */
type T =
	| { k: 'row'; table: Table; prefix: string }
	| { k: 'item'; table: Table }
	| { k: 'source'; table: Table }
	| { k: 'ns'; name: string }
	| { k: 'value'; kind: string };

const ANY: T = { k: 'value', kind: 'any' };

let docsCache: ExprDoc[] = [];
engine
	.expressionDocs()
	.then((d) => (docsCache = d))
	.catch(() => {});

export function docs(): ExprDoc[] {
	return docsCache;
}

/** Texte de l'expression avant le curseur ; en mode texte, seulement dans un bloc `{{ … }}`. */
export function expressionBefore(text: string, template: boolean): string | null {
	if (!template) return text;
	const open = text.lastIndexOf('{{');
	if (open < 0 || text.indexOf('}}', open) >= 0) return null;
	return text.slice(open + 2);
}

/** Expression qui précède un `.` (en équilibrant parenthèses, crochets et chaînes). */
function receiverBefore(text: string, end: number): string {
	let i = end;
	let depth = 0;
	const skipString = (q: string) => {
		i--;
		while (i > 0 && text[i - 1] !== q) i--;
		i--;
	};
	while (i > 0) {
		const c = text[i - 1];
		if (c === ')' || c === ']' || c === '}') {
			depth++;
			i--;
		} else if (c === '(' || c === '[' || c === '{') {
			if (depth === 0) break;
			depth--;
			i--;
		} else if (c === '"' || c === "'" || c === '`') {
			skipString(c);
		} else if (depth > 0 || /[\p{L}\p{N}_$.?]/u.test(c)) {
			i--;
		} else {
			break;
		}
	}
	return text.slice(Math.max(i, 0), end).trim();
}

function findTable(ctx: ExprContext, name: string): Table | null {
	return ctx.nodes.find((n) => n.name === name) ?? (ctx.input?.name === name ? ctx.input : null);
}

function columnKind(table: Table, name: string): string {
	return kindOf(table.columns.find((c) => c.name === name)?.type);
}

/** Type des valeurs après une méthode ou une propriété d'après la documentation. */
function returns(on: string[], name: string, call: boolean): T {
	const d = docsCache.find(
		(d) => d.name === name && on.includes(d.on) && (call ? d.kind !== 'property' : d.kind !== 'method'),
	);
	if (!d) return ANY;
	if (d.returns === 'item') return ANY;
	return { k: 'value', kind: d.returns };
}

/** Déduit le type d'une expression simple (`$json.nom`, `$('X').first().json`…). */
export function typeOf(src: string, ctx: ExprContext): T {
	let s = src.trim();
	let t: T;
	const take = (re: RegExp): RegExpMatchArray | null => {
		const m = s.match(re);
		if (m) s = s.slice(m[0].length);
		return m;
	};
	let m: RegExpMatchArray | null;
	if ((m = take(/^\$\(\s*(['"])(.*?)\1\s*\)/))) {
		const table = findTable(ctx, m[2]);
		if (!table) return ANY;
		t = { k: 'source', table };
	} else if ((m = take(/^\$node\s*\[\s*(['"])(.*?)\1\s*\]/))) {
		const table = findTable(ctx, m[2]);
		if (!table) return ANY;
		t = { k: 'item', table };
	} else if ((m = take(/^(['"`])(?:[^\\]|\\.)*?\1/))) {
		t = { k: 'value', kind: 'string' };
	} else if ((m = take(/^\d[\d_.]*/))) {
		t = { k: 'value', kind: 'number' };
	} else if (s.startsWith('[')) {
		return { k: 'value', kind: 'list' };
	} else if (s.startsWith('(') || s.startsWith('{')) {
		return ANY;
	} else if ((m = take(/^[\p{L}_$][\p{L}\p{N}_$]*/u))) {
		const name = m[0];
		if (name === '$json') {
			if (!ctx.input) return ANY;
			t = { k: 'row', table: ctx.input, prefix: '' };
		} else if (name === '$input') {
			if (!ctx.input) return ANY;
			t = { k: 'source', table: ctx.input };
		} else if (name === '$now' || name === '$today') {
			t = { k: 'value', kind: 'date' };
		} else if (name === '$index') {
			t = { k: 'value', kind: 'number' };
		} else if (['Math', 'JSON', 'Object'].includes(name)) {
			t = { k: 'ns', name };
		} else if (s.trimStart().startsWith('(')) {
			// Appel d'une fonction globale : Number(…), $if(…)…
			const call = take(/^\s*\((?:[^()]|\([^()]*\))*\)/);
			if (!call) return ANY;
			t = returns(['global', 'legacy'], name, true);
		} else if (ctx.input && ctx.input.columns.some((c) => c.name === name)) {
			t = { k: 'value', kind: columnKind(ctx.input, name) };
		} else {
			const v = ctx.vars.find(([n]) => n === name);
			if (!v) return ANY;
			t = { k: 'value', kind: typeof v[1] === 'number' ? 'number' : 'string' };
		}
	} else {
		return ANY;
	}

	// Accès successifs : .nom, ?.nom, (…), [ … ]
	while (s.length) {
		s = s.trimStart();
		let name: string | null = null;
		if ((m = take(/^\??\.\s*([\p{L}_$][\p{L}\p{N}_$]*)/u))) name = m[1];
		else if ((m = take(/^\[\s*(['"])(.*?)\1\s*\]/))) name = m[2];
		else if (take(/^\[[^\]]*\]/)) {
			t = t.k === 'value' && t.kind === 'list' ? ANY : ANY;
			continue;
		} else return ANY;
		const call = !!take(/^\s*\((?:[^()]|\([^()]*\))*\)/);
		t = member(t, name, call);
		if (t === ANY && s.trim()) return ANY;
	}
	return t;
}

function member(t: T, name: string, call: boolean): T {
	switch (t.k) {
		case 'row': {
			const full = t.prefix + name;
			if (t.table.columns.some((c) => c.name === full)) return { k: 'value', kind: columnKind(t.table, full) };
			if (t.table.columns.some((c) => c.name.startsWith(`${full}.`))) return { k: 'row', table: t.table, prefix: `${full}.` };
			return ANY;
		}
		case 'item':
			return name === 'json' ? { k: 'row', table: t.table, prefix: '' } : ANY;
		case 'source':
			if (name === 'first' || name === 'last' || name === 'item') return { k: 'item', table: t.table };
			if (name === 'all') return { k: 'value', kind: 'list' };
			return ANY;
		case 'ns':
			return returns([t.name], name, call);
		case 'value': {
			const on = t.kind === 'any' ? [...VALUE_KINDS, 'any'] : [t.kind, 'any'];
			return returns(on, name, call);
		}
	}
}

function docInfo(d: ExprDoc): () => Node {
	return () => {
		const root = document.createElement('div');
		root.className = 'cm-doc';
		const sig = document.createElement('code');
		sig.className = 'cm-doc-sig';
		sig.textContent = `${d.kind === 'method' ? '.' : ''}${d.name}${d.args ?? ''}${d.returns && d.returns !== 'any' ? ` → ${kindLabels[d.returns] ?? d.returns}` : ''}`;
		const text = document.createElement('p');
		text.textContent = d.info;
		root.append(sig, text);
		if (d.example) {
			const ex = document.createElement('code');
			ex.className = 'cm-doc-example';
			ex.textContent = d.result ? `${d.example}  // ${d.result}` : d.example;
			root.append(ex);
		}
		return root;
	};
}

/** Insère `nom()` en plaçant le curseur entre les parenthèses s'il y a des arguments. */
function callApply(name: string, args: string | undefined) {
	return (view: EditorView, _c: Completion, from: number, to: number) => {
		const next = view.state.sliceDoc(to, to + 1);
		const insert = next === '(' ? name : `${name}()`;
		const inside = args && args !== '()' ? 1 : 2;
		view.dispatch({
			changes: { from, to, insert },
			selection: { anchor: from + name.length + (next === '(' ? 1 : inside) },
		});
	};
}

function docOption(d: ExprDoc, section: CompletionSection, boost = 0): Completion {
	const callable = d.kind === 'method' || d.kind === 'function';
	return {
		label: d.name,
		detail: callable ? (d.args ?? '()') : kindLabels[d.returns] ?? d.returns,
		info: docInfo(d),
		type: d.kind === 'constant' ? 'constant' : d.kind,
		section,
		boost,
		apply: callable && d.name !== '$' ? callApply(d.name, d.args) : undefined,
	};
}

async function columnOptions(table: Table, prefix: string, dotFrom: number | null): Promise<Completion[]> {
	const sample = await sampleRow(table.bytes);
	const seen = new Set<string>();
	const out: Completion[] = [];
	for (const c of table.columns as ColumnInfo[]) {
		if (!c.name.startsWith(prefix)) continue;
		const rest = c.name.slice(prefix.length);
		const segment = rest.split('.')[0];
		if (seen.has(segment)) continue;
		seen.add(segment);
		const nested = rest.includes('.');
		const kind = nested ? 'object' : kindOf(c.type);
		const value = nested ? undefined : sample[c.name];
		const option: Completion = {
			label: segment,
			detail: `${kindLabels[kind]}${value !== undefined ? ` · ${shortValue(value, 24)}` : ''}`,
			type: 'property',
			section: sections.columns,
			boost: 10,
		};
		// Nom avec espaces : `$json["code postal"]` à la place de `.code postal`.
		if (!IDENT.test(segment) && dotFrom !== null) {
			option.apply = (view, _c, _from, to) => {
				const insert = `[${JSON.stringify(segment)}]`;
				view.dispatch({ changes: { from: dotFrom, to, insert }, selection: { anchor: dotFrom + insert.length } });
			};
		}
		out.push(option);
	}
	return out;
}

function memberOptions(t: T, dotFrom: number): Promise<Completion[]> | Completion[] {
	switch (t.k) {
		case 'row':
			return columnOptions(t.table, t.prefix, dotFrom);
		case 'item':
			return [{ label: 'json', detail: 'colonnes de la ligne', type: 'property', section: sections.columns, boost: 10 }];
		case 'source':
			return docsCache.filter((d) => d.on === 'input').map((d) => docOption(d, sections.methods, d.name === 'first' ? 2 : 0));
		case 'ns':
			return docsCache.filter((d) => d.on === t.name).map((d) => docOption(d, sections.functions));
		case 'value': {
			const kinds = t.kind === 'any' ? VALUE_KINDS : [t.kind];
			const seen = new Set<string>();
			return docsCache
				.filter((d) => (kinds.includes(d.on) || d.on === 'any') && (d.kind === 'method' || d.kind === 'property'))
				.filter((d) => !seen.has(d.name) && !!seen.add(d.name))
				.map((d) => {
					const o = docOption(d, sections.methods);
					if (t.kind === 'any' && d.on !== 'any') o.detail = `${o.detail} · ${kindLabels[d.on] ?? d.on}`;
					return o;
				});
		}
	}
}

async function globalOptions(ctx: ExprContext): Promise<Completion[]> {
	const out: Completion[] = [];
	if (ctx.input) {
		const sample = await sampleRow(ctx.input.bytes);
		for (const c of ctx.input.columns) {
			const ref = c.name.split('.').every((p) => IDENT.test(p)) ? `$json.${c.name}` : `$json[${JSON.stringify(c.name)}]`;
			const value = sample[c.name];
			out.push({
				label: ref,
				detail: `${kindLabels[kindOf(c.type)]}${value !== undefined ? ` · ${shortValue(value, 24)}` : ''}`,
				type: 'property',
				section: sections.columns,
				boost: 5,
			});
		}
	}
	for (const d of docsCache.filter((d) => d.on === 'global')) {
		if ((d.name === '$json' || d.name === '$input') && !ctx.input) continue;
		if (d.name === '$' && !ctx.allowNodes) continue;
		const o = docOption(d, d.name.startsWith('$') ? sections.vars : sections.functions, d.name === '$json' ? 3 : 0);
		if (d.name === '$') {
			o.label = "$('…')";
			o.apply = (view, _c, from, to) => {
				view.dispatch({ changes: { from, to, insert: "$('" }, selection: { anchor: from + 3 } });
			};
		}
		out.push(o);
	}
	for (const [name, value] of ctx.vars) {
		out.push({ label: name, detail: shortValue(value, 24), type: 'variable', section: sections.vars, boost: 4 });
	}
	for (const d of docsCache.filter((d) => d.on === 'legacy')) out.push(docOption(d, sections.legacy, -10));
	for (const atom of ['true', 'false', 'null']) out.push({ label: atom, type: 'keyword', boost: -20 });
	return out;
}

/** Source d'autocomplétion CodeMirror (le contexte est lu à chaque appel). */
export function completionSource(getContext: () => ExprContext, template: boolean) {
	return async (cx: CompletionContext): Promise<CompletionResult | null> => {
		const ctx = getContext();
		const line = cx.state.sliceDoc(0, cx.pos);
		const before = expressionBefore(line, template);
		if (before === null) return null;
		const base = cx.pos - before.length;

		// $('Nom du nœud
		let m = before.match(/\$\(\s*(['"])([^'"]*)$/);
		if (m) {
			if (!ctx.allowNodes) return null;
			const quote = m[1];
			const after = cx.state.sliceDoc(cx.pos, cx.pos + 2);
			return {
				from: cx.pos - m[2].length,
				options: ctx.nodes.map((n) => ({
					label: n.name,
					detail: `${n.rows} ligne(s)`,
					type: 'class',
					section: sections.nodes,
					apply: after.startsWith(quote) ? n.name : `${n.name}${quote})`,
				})),
				validFor: /^[^'"]*$/,
			};
		}
		// $json["code postal
		m = before.match(/\$json\[\s*(['"])([^'"]*)$/);
		if (m && ctx.input) {
			const quote = m[1];
			return {
				from: cx.pos - m[2].length,
				options: ctx.input.columns.map((c) => ({
					label: c.name,
					detail: kindLabels[kindOf(c.type)],
					type: 'property',
					section: sections.columns,
					apply: cx.state.sliceDoc(cx.pos, cx.pos + 1) === quote ? c.name : `${c.name}${quote}]`,
				})),
				validFor: /^[^'"]*$/,
			};
		}
		// Pas de suggestion dans une chaîne ou un commentaire.
		const quotes = (before.match(/(?<!\\)['"`]/g) ?? []).length;
		if (quotes % 2 === 1 || /\/\/[^\n]*$/.test(before)) return null;

		const word = before.match(/[\p{L}\p{N}_$]*$/u)?.[0] ?? '';
		const wordFrom = cx.pos - word.length;
		const beforeWord = before.slice(0, before.length - word.length);
		const dot = beforeWord.match(/\??\.\s*$/);
		if (dot) {
			const dotFrom = base + beforeWord.length - dot[0].length;
			const receiver = receiverBefore(beforeWord, beforeWord.length - dot[0].length);
			if (!receiver || /^\d+$/.test(receiver)) return null;
			const options = await memberOptions(typeOf(receiver, ctx), dotFrom);
			return { from: wordFrom, options, validFor: /^[\p{L}\p{N}_$]*$/u };
		}
		// Après « {{ » (ou au début), les suggestions s'ouvrent seules ; sinon il faut un début de mot.
		if (!word && !cx.explicit && !/^\s*$/.test(before)) return null;
		if (/\d/.test(word[0] ?? '')) return null;
		return { from: wordFrom, options: await globalOptions(ctx), validFor: /^[\p{L}\p{N}_$]*$/u };
	};
}
