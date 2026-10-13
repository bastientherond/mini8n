-- Données des exécutions (aperçu des lignes produites) conservées dans l'historique.
-- false : seuls le statut, les comptes de lignes, les colonnes et les erreurs sont gardés
-- (workflows traitant des données personnelles).
ALTER TABLE workflows ADD COLUMN keep_data BOOLEAN NOT NULL DEFAULT true;
