// Coloration syntaxique des expressions (sous-ensemble de JavaScript) et des
// textes à expressions `{{ … }}`, pour CodeMirror.

import { HighlightStyle, StreamLanguage, syntaxHighlighting, type StringStream } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

interface State {
	/** Mode texte : à l'intérieur d'un bloc `{{ … }}`. */
	inExpr: boolean;
	inComment: boolean;
	template: boolean;
}

const OPERATOR = /^(===|!==|==|!=|<=|>=|&&|\|\||\?\?|\?\.|=>|\*\*|[-+*/%<>!?:=])/;
const IDENT = /^[\p{L}_$][\p{L}\p{N}_$]*/u;

function code(stream: StringStream, state: State): string | null {
	if (state.inComment) {
		if (stream.match(/^.*?\*\//)) state.inComment = false;
		else stream.skipToEnd();
		return 'comment';
	}
	if (stream.eatSpace()) return null;
	if (stream.match('//')) {
		stream.skipToEnd();
		return 'comment';
	}
	if (stream.match('/*')) {
		state.inComment = true;
		return 'comment';
	}
	if (stream.match(/^"(?:[^"\\]|\\.)*"?/) || stream.match(/^'(?:[^'\\]|\\.)*'?/) || stream.match(/^`(?:[^`\\]|\\.)*`?/)) {
		return 'string';
	}
	if (stream.match(/^\d[\d_]*(\.\d+)?([eE][+-]?\d+)?/) || stream.match(/^\.\d+/)) return 'number';
	if (stream.match(/^(true|false|null|undefined)(?![\p{L}\p{N}_$])/u)) return 'atom';
	const before = stream.string.slice(0, stream.pos);
	const ident = stream.match(IDENT) as RegExpMatchArray | null;
	if (ident) {
		const name = ident[0];
		if (name.startsWith('$')) return 'special';
		if (/\.\s*$/.test(before)) return /^\s*\(/.test(stream.string.slice(stream.pos)) ? 'method' : 'property';
		if (/^\s*\(/.test(stream.string.slice(stream.pos))) return 'function';
		if (['Math', 'JSON', 'Object', 'Number', 'String', 'Boolean'].includes(name)) return 'namespace';
		return 'variable';
	}
	if (stream.match(OPERATOR)) return 'operator';
	stream.next();
	return 'punctuation';
}

function language(template: boolean) {
	return StreamLanguage.define<State>({
		name: template ? 'mini8n-template' : 'mini8n-expression',
		startState: () => ({ inExpr: !template, inComment: false, template }),
		copyState: (s) => ({ ...s }),
		token(stream, state) {
			if (state.template) {
				if (!state.inExpr) {
					if (stream.match('{{')) {
						state.inExpr = true;
						return 'brace';
					}
					// Texte libre jusqu'au prochain `{{`.
					while (!stream.eol() && !stream.match('{{', false)) stream.next();
					return null;
				}
				if (!state.inComment && stream.match('}}')) {
					state.inExpr = false;
					return 'brace';
				}
			}
			return code(stream, state);
		},
		tokenTable: {
			special: t.special(t.variableName),
			method: t.function(t.propertyName),
			property: t.propertyName,
			function: t.function(t.variableName),
			namespace: t.namespace,
			variable: t.variableName,
			brace: t.brace,
		},
		// En mode texte, `{` est géré à part : `{{` ouvre un bloc `{{ … }}`.
		languageData: { closeBrackets: { brackets: template ? ['(', '[', "'", '"', '`'] : ['(', '[', '{', "'", '"', '`'] } },
	});
}

export const expressionLanguage = language(false);
export const templateLanguage = language(true);

/** Couleurs : variables CSS définies dans app.css (thèmes clair et sombre). */
export const highlight = syntaxHighlighting(
	HighlightStyle.define([
		{ tag: t.special(t.variableName), color: 'var(--code-special)', fontWeight: '600' },
		{ tag: [t.function(t.propertyName), t.function(t.variableName)], color: 'var(--code-function)' },
		{ tag: t.propertyName, color: 'var(--code-property)' },
		{ tag: t.namespace, color: 'var(--code-namespace)' },
		{ tag: [t.string, t.special(t.string)], color: 'var(--code-string)' },
		{ tag: [t.number, t.atom, t.bool, t.null], color: 'var(--code-number)' },
		{ tag: [t.keyword, t.operator, t.compareOperator, t.logicOperator, t.arithmeticOperator], color: 'var(--code-operator)' },
		{ tag: t.comment, color: 'var(--code-comment)', fontStyle: 'italic' },
		{ tag: t.brace, color: 'var(--code-brace)', fontWeight: '700' },
		{ tag: t.typeName, color: 'var(--code-namespace)' },
	]),
);
