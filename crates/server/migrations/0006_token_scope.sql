-- Portée d'un jeton d'IA :
--   lecture : consultation seulement (workflows, données, historique) ;
--   test    : + modification des workflows inactifs et exécutions en simulation
--             (nœuds d'envoi neutralisés, lectures de bases en lecture seule) ;
--   complet : tout, y compris les envois réels. Jetons existants : complet (inchangés).
ALTER TABLE api_tokens ADD COLUMN scope TEXT NOT NULL DEFAULT 'complet'
    CHECK (scope IN ('lecture', 'test', 'complet'));
