const $ = id => document.getElementById(id);
let session = null;
let configured = false;
let revokeId = null;
let deleteId = null;
let editId = null;
let modelsLoaded = false;
function notify(message, error = false) { $('notice').textContent = message; $('notice').classList.toggle('error', error); }
async function api(path, { method = 'GET', body } = {}) {
  const headers = {};
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (session?.csrf) headers['X-CSRF-Token'] = session.csrf;
  const response = await fetch('/console/api/' + path, { method, credentials: 'same-origin', headers, ...(body !== undefined ? { body: JSON.stringify(body) } : {}) });
  const data = await response.json();
  if (!response.ok) {
    if (response.status === 401 && session) { session = null; await render(); }
    throw new Error(data.error?.message || 'Request failed.');
  }
  return data;
}
function example() { return `curl '${session.apiBase}/chat/completions' \
  -H 'Authorization: Bearer YOUR_API_KEY' \
  -H 'Content-Type: application/json' \
  -d '{"model":"MODEL_ID","messages":[{"role":"user","content":"Hello"}]}'`; }
async function render() {
  $('auth-view').hidden = Boolean(session);
  $('dashboard').hidden = !session;
  $('identity').hidden = !session;
  if (!session) {
    $('auth-title').textContent = configured ? 'Welcome back.' : 'Create your owner account.';
    $('auth-caption').textContent = configured ? 'Sign in to manage your private API keys.' : 'Choose a username and password. Account creation closes after this step. Create your account before making the console publicly accessible.';
    $('password-help').hidden = configured;
    $('password').minLength = configured ? 1 : 12;
    $('password').autocomplete = configured ? 'current-password' : 'new-password';
    $('auth-submit').textContent = configured ? 'Sign in' : 'Create owner account';
    return;
  }
  $('owner-name').textContent = session.username;
  $('api-base').value = session.apiBase;
  $('example').textContent = example();
  await loadKeys();
}
const date = value => value ? new Date(value).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : 'Never';
async function loadKeys() {
  const { keys } = await api('keys');
  $('keys-body').replaceChildren();
  $('empty-state').hidden = keys.length !== 0;
  for (const key of keys.slice().reverse()) {
    const row = document.createElement('tr');
    for (const text of [key.name, key.prefix + '…', date(key.createdAt), date(key.expiresAt), date(key.lastUsedAt)]) {
      const td = document.createElement('td');
      td.textContent = text;
      if (text === key.prefix + '…') td.className = 'key-prefix';
      row.append(td);
    }
    const status = document.createElement('td');
    const badge = document.createElement('span');
    const isRevoked = Boolean(key.revokedAt);
    const isExpired = Boolean(key.isExpired);
    badge.className = 'badge' + (isRevoked ? ' revoked' : (isExpired ? ' expired' : ''));
    badge.textContent = isRevoked ? 'Revoked' : (isExpired ? 'Expired' : 'Active');
    status.append(badge);
    row.append(status);

    const action = document.createElement('td');
    action.style.display = 'flex';
    action.style.gap = '6px';

    if (!isRevoked && !isExpired) {
      const revBtn = document.createElement('button');
      revBtn.textContent = 'Revoke';
      revBtn.setAttribute('aria-label', 'Revoke ' + key.name);
      revBtn.addEventListener('click', () => {
        revokeId = key.id;
        $('revoke-caption').textContent = `Apps using “${key.name}” will immediately lose access.`;
        $('revoke-dialog').showModal();
      });
      action.append(revBtn);
    }

    const editBtn = document.createElement('button');
    editBtn.textContent = 'Edit Date';
    editBtn.className = 'quiet';
    editBtn.setAttribute('aria-label', 'Edit ' + key.name);
    editBtn.addEventListener('click', () => {
      editId = key.id;
      $('edit-name').value = key.name;
      $('edit-expiry-date').value = key.expiresAt ? key.expiresAt.substring(0, 10) : '';
      $('edit-dialog').showModal();
    });
    action.append(editBtn);

    const delBtn = document.createElement('button');
    delBtn.textContent = 'Delete';
    delBtn.className = 'quiet danger';
    delBtn.style.color = '#c00';
    delBtn.setAttribute('aria-label', 'Delete ' + key.name);
    delBtn.addEventListener('click', () => {
      deleteId = key.id;
      $('delete-caption').textContent = `Delete “${key.name}” permanently? It will be removed from your server.`;
      $('delete-dialog').showModal();
    });
    action.append(delBtn);

    row.append(action);
    $('keys-body').append(row);
  }
}
async function copy(text, button) {
  try { await navigator.clipboard.writeText(text); const original = button.textContent; button.textContent = 'Copied'; setTimeout(() => { button.textContent = original; }, 1800); }
  catch { notify('Clipboard unavailable. Select the text and copy it manually.', true); }
}
function openCreate() {
  $('key-name').value = '';
  $('key-expiry').value = '0';
  $('custom-date-wrap').hidden = true;
  $('key-custom-date').value = '';
  $('create-dialog').showModal();
  $('key-name').focus();
}
$('open-create').onclick = openCreate;
$('empty-create').onclick = openCreate;
$('key-expiry').onchange = () => {
  $('custom-date-wrap').hidden = $('key-expiry').value !== 'custom';
};
for (const button of document.querySelectorAll('.close-create')) button.onclick = () => $('create-dialog').close();
for (const button of document.querySelectorAll('.close-edit')) button.onclick = () => $('edit-dialog').close();
$('cancel-delete').onclick = () => $('delete-dialog').close();
$('confirm-delete').onclick = async () => {
  $('confirm-delete').disabled = true;
  try {
    await api('keys/' + deleteId + '?permanent=true', { method: 'DELETE' });
    $('delete-dialog').close();
    await loadKeys();
    notify('Key deleted permanently. Users can no longer use it.');
  } catch (error) {
    $('delete-dialog').close();
    notify(error.message, true);
  } finally {
    $('confirm-delete').disabled = false;
  }
};
$('edit-form').onsubmit = async event => {
  event.preventDefault();
  $('edit-submit').disabled = true;
  try {
    const expiryVal = $('edit-expiry-date').value;
    const body = {
      name: $('edit-name').value,
      expiresAt: expiryVal ? new Date(expiryVal + 'T23:59:59Z').toISOString() : null
    };
    await api('keys/' + editId, { method: 'PATCH', body });
    $('edit-dialog').close();
    await loadKeys();
    notify('Key updated successfully.');
  } catch (error) {
    $('edit-dialog').close();
    notify(error.message, true);
  } finally {
    $('edit-submit').disabled = false;
  }
};
$('auth-form').onsubmit = async event => {
  event.preventDefault(); const button = $('auth-submit'); button.disabled = true; notify('');
  try {
    const credentials = { username: $('username').value, password: $('password').value };
    if (!configured) { await api('setup', { method: 'POST', body: credentials }); configured = true; }
    session = await api('login', { method: 'POST', body: credentials }); $('password').value = ''; await render(); notify('Signed in to your private console.');
  } catch (error) {
    try { const state = await api('session'); configured = state.configured; session = state.authenticated ? state : null; } catch {}
    await render();
    notify(error.message, true);
  } finally { button.disabled = false; }
};
$('logout').onclick = async () => { try { await api('logout', { method: 'POST', body: {} }); session = null; modelsLoaded = false; $('models-list').replaceChildren(); $('models-details').open = false; $('new-key').value = ''; for (const dialog of document.querySelectorAll('dialog')) dialog.close(); await render(); notify('Signed out.'); } catch (error) { notify(error.message, true); } };
$('create-form').onsubmit = async event => {
  event.preventDefault();
  $('create-submit').disabled = true;
  try {
    const expirySelect = $('key-expiry').value;
    const body = { name: $('key-name').value };
    if (expirySelect === 'custom') {
      const customDate = $('key-custom-date').value;
      if (customDate) {
        body.expiresAt = new Date(customDate + 'T23:59:59Z').toISOString();
      }
    } else {
      const days = parseInt(expirySelect, 10);
      if (days > 0) body.expiresInDays = days;
    }

    const data = await api('keys', { method: 'POST', body });
    $('create-dialog').close();
    $('new-key').value = data.key;
    $('secret-dialog').showModal();
    await loadKeys();
  } catch (error) {
    $('create-dialog').close();
    notify(error.message, true);
  } finally {
    $('create-submit').disabled = false;
  }
};
$('secret-dialog').addEventListener('close', () => { $('new-key').value = ''; });
$('close-secret').onclick = () => { $('new-key').value = ''; $('secret-dialog').close(); };
$('copy-key').onclick = () => copy($('new-key').value, $('copy-key'));
$('copy-url').onclick = () => copy($('api-base').value, $('copy-url'));
$('copy-example').onclick = () => copy($('example').textContent, $('copy-example'));
$('cancel-revoke').onclick = () => $('revoke-dialog').close();
$('confirm-revoke').onclick = async () => { $('confirm-revoke').disabled = true; try { await api('keys/' + revokeId, { method: 'DELETE' }); $('revoke-dialog').close(); await loadKeys(); notify('Key revoked. Apps using it can no longer make requests.'); } catch (error) { $('revoke-dialog').close(); notify(error.message, true); } finally { $('confirm-revoke').disabled = false; } };
async function loadModels() {
  $('refresh-models').disabled = true; $('models-status').textContent = 'Checking connected accounts…';
  try { const { data } = await api('models'); $('models-list').replaceChildren(); for (const model of data) { const li = document.createElement('li'); li.textContent = model.id; $('models-list').append(li); } $('models-status').textContent = data.length ? 'Copy a model ID into your app’s model field.' : 'No models returned. Connect an Antigravity account on your server first.'; modelsLoaded = true; }
  catch (error) { $('models-status').textContent = error.message; } finally { $('refresh-models').disabled = false; }
}
$('models-details').addEventListener('toggle', () => { if ($('models-details').open && !modelsLoaded) loadModels(); });
$('refresh-models').onclick = loadModels;
(async () => { try { const state = await api('session'); configured = state.configured; session = state.authenticated ? state : null; await render(); } catch (error) { notify(error.message, true); } })();
