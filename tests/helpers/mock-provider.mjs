import http from 'node:http';

/**
 * Small OpenAI-compatible provider used to exercise real SDK streaming, auth,
 * cancellation, and retry behavior without making network requests.
 */
export async function startMockProvider(options = {}) {
  const state = {
    key: options.key ?? 'test-secret-key',
    mode: options.mode ?? 'success',
    chunks: options.chunks ?? ['Hello', ' from', ' mock'],
    delayMs: options.delayMs ?? 0,
    errorStatus: options.errorStatus ?? 401,
    errorMessage: options.errorMessage ?? 'Invalid API key',
    requests: [],
    aborted: 0,
  };

  function authorize(req) {
    return req.headers.authorization === `Bearer ${state.key}`;
  }

  function errorBody(status, message) {
    return JSON.stringify({
      error: {
        message,
        type: status === 401 ? 'authentication_error' : 'server_error',
        code: status === 401 ? 'invalid_api_key' : 'mock_error',
      },
    });
  }

  const server = http.createServer((req, res) => {
    let body = '';
    req.on('data', chunk => { body += chunk; });
    req.on('end', () => {
      let parsed = {};
      try { parsed = JSON.parse(body || '{}'); } catch { /* malformed body */ }
      state.requests.push({
        method: req.method,
        url: req.url,
        authorization: req.headers.authorization,
        body: parsed,
      });

      if (req.url?.endsWith('/models')) {
        if (!authorize(req)) {
          res.writeHead(401, { 'content-type': 'application/json' });
          res.end(errorBody(401, state.errorMessage));
          return;
        }
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ data: [{ id: 'mock-model', object: 'model' }] }));
        return;
      }

      if (req.url?.endsWith('/chat/completions')) {
        if (state.mode === 'unauthorized' || !authorize(req)) {
          res.writeHead(state.errorStatus, { 'content-type': 'application/json' });
          res.end(errorBody(state.errorStatus, state.errorMessage));
          return;
        }
        if (state.mode === 'unauthorized-once') {
          state.mode = 'success';
          res.writeHead(401, { 'content-type': 'application/json' });
          res.end(errorBody(401, state.errorMessage));
          return;
        }
        if (state.mode === 'error') {
          res.writeHead(500, { 'content-type': 'application/json' });
          res.end(errorBody(500, 'Mock provider failure'));
          return;
        }

        const stream = parsed.stream !== false;
        if (!stream) {
          res.writeHead(200, { 'content-type': 'application/json' });
          res.end(JSON.stringify({
            id: 'mock-completion',
            object: 'chat.completion',
            created: Math.floor(Date.now() / 1000),
            model: parsed.model ?? 'mock-model',
            choices: [{
              index: 0,
              message: { role: 'assistant', content: state.chunks.join('') },
              finish_reason: 'stop',
            }],
            usage: { prompt_tokens: 1, completion_tokens: state.chunks.length, total_tokens: state.chunks.length + 1 },
          }));
          return;
        }

        res.writeHead(200, {
          'content-type': 'text/event-stream',
          'cache-control': 'no-cache',
          connection: 'keep-alive',
        });
        if (['web-tool','mcp-tool'].includes(state.mode) && !parsed.messages?.some(message => message.role === 'tool')) {
          const chunk = (delta, finish_reason = null) => res.write(`data: ${JSON.stringify({id:'mock-tool',object:'chat.completion.chunk',created:1,model:parsed.model,choices:[{index:0,delta,finish_reason}]})}\n\n`);
          chunk({role:'assistant', tool_calls:[{index:0,id:'call-web-1',type:'function',function:state.mode==='mcp-tool'?{name:'tools',arguments:JSON.stringify({operation:'call',server:'fixture',tool:'echo',arguments:{text:'Pi executed real MCP'}})}:{name:'tools',arguments:JSON.stringify({operation:'call',server:'talo-research',tool:'web_search',arguments:{query:'Rust documentation'}})}}]});
          chunk({}, 'tool_calls');
          res.end('data: [DONE]\n\n');
          return;
        }
        let timer;
        let index = 0;
        let closed = false;
        const send = payload => {
          if (closed) return;
          res.write(`data: ${JSON.stringify(payload)}\n\n`);
        };
        const finish = () => {
          if (closed) return;
          send({
            id: 'mock-completion',
            object: 'chat.completion.chunk',
            created: Math.floor(Date.now() / 1000),
            model: parsed.model ?? 'mock-model',
            choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: state.chunks.length, total_tokens: state.chunks.length + 1 },
          });
          res.write('data: [DONE]\n\n');
          closed = true;
          res.end();
        };
        const tick = () => {
          if (closed) return;
          if (index >= state.chunks.length) {
            finish();
            return;
          }
          send({
            id: 'mock-completion',
            object: 'chat.completion.chunk',
            created: Math.floor(Date.now() / 1000),
            model: parsed.model ?? 'mock-model',
            choices: [{ index: 0, delta: { content: state.chunks[index] }, finish_reason: null }],
          });
          index += 1;
          timer = setTimeout(tick, state.delayMs);
        };
        res.on('close', () => {
          if (!closed) {
            state.aborted += 1;
            closed = true;
            clearTimeout(timer);
          }
        });
        tick();
        return;
      }

      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { message: 'Not found' } }));
    });
  });

  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  const port = typeof address === 'object' && address ? address.port : 0;

  return {
    port,
    url: `http://127.0.0.1:${port}/v1`,
    state,
    configure(next) {
      Object.assign(state, next);
    },
    close() {
      return new Promise(resolve => server.close(() => resolve()));
    },
  };
}
