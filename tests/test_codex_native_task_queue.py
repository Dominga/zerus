"""Keyless native Codex MCP hooks and exact-thread delivery, with private state.

The mocked MCP tool returns a synthetic id and creates no task or agent.
"""
import http.server
import json
import os
from pathlib import Path
import shlex
import threading
import time
import unittest

import test_clear_context_native as native
import test_codex_async_questions_native as wire
import test_codex_input_native as input_native
import test_input

class Provider(wire.Provider):
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers.get('Content-Length','0'))))
  outputs=[x for x in body.get('input',[]) if x.get('type')=='function_call_output' and x.get('call_id')=='synthetic_mcp']
  self.server.results.extend(outputs)
  self.server.offered.append(sorted(wire.offered(body.get('tools',[])+[t for x in body.get('input',[]) if x.get('type')=='additional_tools' for t in x.get('tools',[])])))
  if outputs:item=dict(type='message',id='fixture_done',role='assistant',status='completed',content=[dict(type='output_text',text='Fixture complete.',annotations=[])])
  else:item=dict(type='function_call',id='fixture_mcp',call_id='synthetic_mcp',namespace='mcp__codex_tui',name='create_thread',arguments='{}',status='completed')
  response=dict(id='resp_fixture',object='response',status='completed',output=[item],usage=dict(input_tokens=1,output_tokens=1,total_tokens=2))
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
  for k,v in [('response.created',dict(response=dict(response,status='in_progress',output=[]))),('response.output_item.added',dict(output_index=0,item=dict(item,status='in_progress'))),('response.output_item.done',dict(output_index=0,item=item)),('response.completed',dict(response=response))]:self.emit(k,**v)

@unittest.skipUnless(os.environ.get('HGS_CODEX_TEST_BIN'), 'Requires isolated native Codex')
class NativeTaskQueue(unittest.TestCase):
 native_features='hooks=true\n'
 script=native.NativeCodexClear.script
 write_record=native.NativeCodexClear.write_record
 tmux=native.NativeCodexClear.tmux
 wait_for=native.NativeCodexClear.wait_for
 tearDown=native.NativeCodexClear.tearDown
 fail=native.NativeCodexClear.fail
 screen=input_native.NativeCodexInput.screen
 payload=test_input.InputTransport.payload
 send=test_input.InputTransport.send
 def setUp(self):
  self.server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Provider)
  self.server.results=[];self.server.offered=[]
  self.addCleanup(self.server.server_close);self.addCleanup(self.server.shutdown)
  threading.Thread(target=self.server.serve_forever,daemon=True).start()
  self.provider_url=f'http://127.0.0.1:{self.server.server_port}/v1'
  native.NativeCodexClear.setUp(self)
 def prepare_native_hooks(self):
  mock=self.root/'mcp.py'
  mock.write_text("import sys,json\nfor line in sys.stdin:\n r=json.loads(line)\n if 'id' not in r:continue\n m=r['method']\n if m=='initialize':v=dict(protocolVersion='2024-11-05',capabilities=dict(tools={}),serverInfo=dict(name='Synthetic native-MCP schema fixture',version='1'))\n elif m=='tools/list':v=dict(tools=[dict(name='create_thread',description='Return a synthetic id; creates no task and runs no agent.',inputSchema=dict(type='object',properties={},additionalProperties=False))])\n elif m=='tools/call':v=dict(content=[dict(type='text',text=json.dumps(dict(threadId='11111111-1111-4111-8111-111111111111')))],isError=False)\n else:v={}\n print(json.dumps(dict(jsonrpc='2.0',id=r['id'],result=v)),flush=True)\n")
  config=self.native_home/'config.toml'
  with config.open('a') as f:f.write('\n[mcp_servers.codex_tui]\ncommand="/usr/bin/python3"\nargs='+json.dumps([str(mock)])+'\n')
  env=dict(HGS_SESSION=self.name,HGS_RUN_ID=self.run_id,HGS_STATE_DIR=str(self.state),HGS_CONFIG_DIR=str(self.root/'config'),HGS_AGENT='codex',PATH=self.env['PATH'])
  command='env '+' '.join(shlex.quote(k+'='+v) for k,v in env.items())+' '+shlex.quote(str(test_input.HGS))+' __state hook'
  (self.native_home/'hooks.json').write_text(json.dumps(dict(hooks={k:[dict(hooks=[dict(type='command',command=command)])] for k in ['SessionStart','PreToolUse','PostToolUse','UserPromptSubmit','Stop']})))

 def until(self,predicate):
  end=time.monotonic()+20
  while time.monotonic()<end:
   value=predicate()
   if value:return value
   time.sleep(.05)
  self.fail('Native task fixture timed out')
 def test_native_mcp_result_keeps_parent_and_send_uses_exact_queue_once(self):
  self.tmux('send-keys','-t',self.pane,'-l','Run the synthetic MCP schema fixture.')
  time.sleep(.5);self.tmux('send-keys','-t',self.pane,'Enter')
  def created():
   r=json.loads(self.record_path.read_text())
   return r if r.get('codex_native_tasks',{}).get('threads',{}).get('11111111-1111-4111-8111-111111111111') and r.get('phase')=='idle' else None
  record=self.until(created)
  self.assertEqual(record['conversation_id'],self.conversation_id)
  self.assertEqual(record['codex_native_tasks']['pending'],{})
  request=self.payload(text='Exact selected task queue fixture')
  receipt=self.send(request)
  self.assertEqual(receipt['transport'],'codex_queue')
  self.assertEqual(receipt['conversation_id'],self.conversation_id)
  self.assertTrue(receipt['native_queued_message_id'])
  self.assertEqual(self.send(request),receipt,'original request UUID never writes the native queue again')
  self.until(lambda:json.loads(self.record_path.read_text()).get('input_seen_at',0)>receipt['submitted_at'])
  self.assertEqual(json.loads(self.record_path.read_text())['conversation_id'],self.conversation_id)
  self.assertNotIn('Exact selected task queue fixture',self.screen().split('Ask Codex to do anything')[-1])
