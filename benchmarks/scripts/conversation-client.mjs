const token = new URL(location.href).searchParams.get('token');
const { ConversationModel } = await import('/conversation-model.mjs?token=' + encodeURIComponent(token));
const model = new ConversationModel(token);
const byId = id => document.getElementById(id);
const transcript = byId('transcript');
const composer = byId('composer');
const status = byId('fixture-status');
let pending = null;
let nonce = 0;

// This acknowledgement is semantic DOM progress. It is explicitly not a presentation timestamp.
function acknowledge(action, generation = model.generation) {
  requestAnimationFrame(() => requestAnimationFrame(() => {
    if (generation !== model.generation) return;
    status.dataset.nonce = String(++nonce);
    status.dataset.action = action;
    status.textContent = `${action}; token ${token}; nonce ${nonce}; conversation ${model.selected === null ? 'home' : model.selected + 1}; draft ${model.draft.length}; reply ${model.reply.length}`;
    document.title = `Conversation fixture ${token}: ${action} ${nonce}`;
  }));
}

function fail(error) {
  status.dataset.action = 'failed';
  status.textContent = `Fixture failed: ${String(error)}`;
  document.title = `BOWSER_PERF_FAILED_${token}`;
}

function cancelPending() {
  pending?.abort();
  pending = null;
}

function home() {
  cancelPending();
  model.home();
  byId('composer-form').hidden = true;
  byId('heading').textContent = 'Choose a synthetic conversation';
  transcript.replaceChildren();
  acknowledge('home_ready');
}

async function openConversation(id) {
  cancelPending();
  const ticket = model.select(id);
  const controller = pending = new AbortController();
  byId('composer-form').hidden = true;
  status.textContent = `Loading conversation ${id + 1}`;
  try {
    const response = await fetch(`/conversation-data?token=${token}&id=${id}`, { signal: controller.signal, cache: 'no-store' });
    if (!response.ok) throw Error(`Transcript HTTP ${response.status}`);
    const data = await response.json();
    if (model.generation !== ticket.generation) return;
    if (data.id !== id || !Array.isArray(data.messages) || data.messages.length !== 120) throw Error('Wrong transcript identity or work count');
    const fragment = document.createDocumentFragment();
    for (let group = 0; group < 6; group++) {
      const section = document.createElement('section');
      section.setAttribute('aria-label', `Messages ${group * 20 + 1} to ${group * 20 + 20}`);
      for (let index = group * 20; index < group * 20 + 20; index++) {
        const message = data.messages[index];
        const article = document.createElement('article');
        const title = document.createElement('h3');
        title.textContent = `${message.role}, message ${index + 1}`;
        const paragraph = document.createElement('p');
        paragraph.textContent = message.text;
        const code = document.createElement('pre');
        code.textContent = message.code;
        article.append(title, paragraph, code);
        section.append(article);
      }
      fragment.append(section);
    }
    const reply = document.createElement('pre');
    reply.id = 'reply';
    reply.setAttribute('aria-label', 'Streamed reply');
    reply.textContent = model.reply;
    fragment.append(reply);
    transcript.replaceChildren(fragment);
    byId('heading').textContent = data.title;
    composer.value = model.draft;
    byId('stream').disabled = false;
    byId('composer-form').hidden = false;
    transcript.scrollTop = transcript.scrollHeight;
    composer.focus();
    acknowledge('conversation_ready', ticket.generation);
  } catch (error) { if (!controller.signal.aborted) fail(error); }
}

async function streamReply() {
  cancelPending();
  const ticket = model.startStream();
  const controller = pending = new AbortController();
  byId('stream').disabled = true;
  byId('reply').textContent = '';
  const text = document.createTextNode('');
  byId('reply').append(text);
  status.textContent = 'Receiving 64 synthetic chunks';
  try {
    const response = await fetch(`/conversation-stream?token=${token}&id=${ticket.id}&generation=${ticket.generation}`, { signal: controller.signal, cache: 'no-store' });
    if (!response.ok || !response.body) throw Error(`Stream HTTP ${response.status}`);
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    let buffered = '';
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffered += value;
      let newline;
      while ((newline = buffered.indexOf('\n')) >= 0) {
        const chunk = JSON.parse(buffered.slice(0, newline));
        buffered = buffered.slice(newline + 1);
        if (!model.acceptChunk(chunk)) {
          if (model.generation !== ticket.generation) return;
          throw Error('Wrong stream identity or extra chunk');
        }
        if (!chunk.done) text.appendData(chunk.text);
      }
    }
    if (model.generation !== ticket.generation) return;
    if (buffered || !model.complete || text.data !== model.reply) throw Error('Incomplete or incorrectly rendered reply');
    acknowledge('stream_complete', ticket.generation);
  } catch (error) { if (!controller.signal.aborted) fail(error); }
  finally { if (model.generation === ticket.generation) byId('stream').disabled = false; }
}

byId('home').addEventListener('click', home);
byId('composer-form').addEventListener('submit', event => event.preventDefault());
composer.addEventListener('input', () => model.saveDraft(composer.value));
byId('stream').addEventListener('click', streamReply);
byId('check').addEventListener('click', () => {
  if (composer.value === model.draft) acknowledge('draft_verified');
  else fail('Draft state differs from the visible input');
});
for (let id = 0; id < 8; id++) {
  const button = document.createElement('button');
  button.textContent = `Conversation ${id + 1}`;
  button.type = 'button';
  button.addEventListener('click', () => openConversation(id));
  byId('conversations').append(button);
}
home();
