// Shared by the synthetic page and its behavioral tests; no Bowser instrumentation.
export class ConversationModel {
  constructor(token) {
    if (typeof token !== 'string' || !/^[a-zA-Z0-9_]{1,80}$/.test(token)) throw Error('Invalid fixture token');
    this.token = token;
    this.selected = null;
    this.generation = 0;
    this.drafts = Array(8).fill('');
    this.replies = Array(8).fill('');
    this.completed = Array(8).fill(false);
    this.streaming = null;
  }

  select(id) {
    if (!Number.isInteger(id) || id < 0 || id > 7) throw Error('Invalid conversation');
    this.selected = id;
    this.streaming = null;
    return { id, generation: ++this.generation };
  }

  home() {
    this.selected = null;
    this.streaming = null;
    this.generation++;
  }

  get draft() { return this.selected === null ? '' : this.drafts[this.selected]; }
  get reply() { return this.selected === null ? '' : this.replies[this.selected]; }
  get complete() { return this.selected !== null && this.completed[this.selected]; }

  saveDraft(text) {
    if (this.selected === null || typeof text !== 'string') throw Error('No selected conversation or invalid draft');
    this.drafts[this.selected] = text;
  }

  startStream() {
    if (this.selected === null) throw Error('No selected conversation');
    const ticket = { token: this.token, id: this.selected, generation: ++this.generation };
    this.replies[this.selected] = '';
    this.completed[this.selected] = false;
    this.streaming = { ...ticket, next: 0 };
    return ticket;
  }

  acceptChunk(chunk) {
    const stream = this.streaming;
    if (!stream || !chunk || chunk.token !== stream.token || chunk.id !== stream.id || chunk.generation !== stream.generation) return false;
    if (chunk.done === true) {
      if (stream.next !== 64 || chunk.chunks !== 64) throw Error('Incomplete reply');
      this.completed[stream.id] = true;
      this.streaming = null;
    } else {
      const expected = `Chunk ${String(stream.next + 1).padStart(2, '0')}: deterministic streamed reply.\n`;
      if (stream.next >= 64 || chunk.sequence !== stream.next || chunk.text !== expected) throw Error('Missing, duplicate or changed chunk');
      this.replies[stream.id] += chunk.text;
      stream.next++;
    }
    return true;
  }
}
