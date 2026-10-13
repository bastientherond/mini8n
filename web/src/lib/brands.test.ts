import assert from 'node:assert/strict';
import { test } from 'node:test';
import { brandOfCredential, brandOfNode, chatService } from './brands.ts';

test('le service est reconnu d’après l’hôte du webhook', () => {
	assert.equal(chatService('https://hooks.slack.com/services/T0/B0/x'), 'Slack');
	assert.equal(chatService('https://discord.com/api/webhooks/1/abc'), 'Discord');
	assert.equal(chatService('https://contoso.webhook.office.com/webhookb2/x'), 'Microsoft Teams');
	assert.equal(chatService('https://prod-12.westeurope.logic.azure.com:443/workflows/x'), 'Microsoft Teams');
	assert.equal(chatService('https://chat.googleapis.com/v1/spaces/X/messages?key=k'), 'Google Chat');
	assert.equal(chatService('https://chat.exemple.com/hooks/abc'), 'Mattermost');
	assert.equal(chatService('https://exemple.com/autre'), null);
	assert.equal(chatService('http://hooks.slack.com/x'), null);
	assert.equal(chatService('https://hooks.slack.com.evil.example/x'), null);
	assert.equal(chatService(''), null);
});

test('marque d’un identifiant et d’un nœud', () => {
	assert.equal(brandOfCredential('postgres'), 'postgres');
	assert.equal(brandOfCredential('chat_webhook', { service: 'Slack' }), 'Slack');
	assert.equal(brandOfCredential('chat_webhook', { service: null }), 'chat_webhook');
	assert.equal(brandOfCredential('inconnu'), 'chat_webhook');
	assert.equal(brandOfNode('postgres_query'), 'postgres');
	assert.equal(brandOfNode('chat_message', 'Discord'), 'Discord');
	assert.equal(brandOfNode('chat_message'), 'chat_webhook');
	assert.equal(brandOfNode('filter'), null);
});
