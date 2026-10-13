// Règles du mot de passe affichées pendant la saisie (le serveur applique les mêmes).

export const PASSWORD_MIN = 12;

export function passwordChecks(password: string, confirm: string) {
	return [
		{ ok: password.length >= PASSWORD_MIN, label: `${PASSWORD_MIN} caractères minimum` },
		{ ok: new Set(password).size >= 5, label: 'Pas trop répétitif' },
		{ ok: password.length > 0 && password === confirm, label: 'Les deux saisies correspondent' },
	];
}
