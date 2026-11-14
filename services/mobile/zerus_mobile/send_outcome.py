"""Negotiated ordinary-send proof. No diagnostic/exit/absence inference."""
import hashlib
import json
import math
import re
import uuid
import unicodedata

SEND = "send_outcome"
READ = "send_outcome_receipt"
OPERATIONS = frozenset({SEND, READ})
MARKERS = frozenset({"native-send-outcome-v2", "delivery-receipt-v1", "mobile-peer-send-outcome-v2", "session-id-selector-v1", "mobile-peer-session-id-v1"})
BINDING_FIELDS = frozenset({"schema", "message_request_id", "computer_id", "machine_id", "session_id", "session", "provider", "run_id", "conversation_id", "native_encoding", "native_digest"})
NEGATIVE_FIELDS = frozenset({"schema", "status", "proof", "request_id", "name", "machine_id", "session_id", "provider", "run_id", "conversation_id", "native_digest", "reason"})
FRAME_REQUIRED = NEGATIVE_FIELDS - {"proof", "reason"}
FRAME_FIELDS = NEGATIVE_FIELDS | {"message_request_id", "agent_id", "submitted_at", "queued"}
MISMATCH = "native send outcome identity did not match; delivery remains unknown"
UNAVAILABLE = "native terminal send outcome ABI is unavailable"


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True, allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def native_digest(payload, encoding):
    if encoding not in {"compact", "python_default"}: raise ValueError(MISMATCH)
    data = json.dumps(payload, sort_keys=True, ensure_ascii=False, allow_nan=False,
                      separators=(",", ":") if encoding == "compact" else (", ", ": ")).encode()
    return hashlib.sha256(data).hexdigest()


def canonical_uuid(value):
    return isinstance(value, str) and str(uuid.UUID(value)) == value and uuid.UUID(value).int != 0


def text(value, maximum, empty=False):
    return isinstance(value, str) and (empty or bool(value)) and len(value.encode()) <= maximum and not any(unicodedata.category(c) == "Cc" for c in value)


def validate_binding(binding):
    if not isinstance(binding, dict) or set(binding) != BINDING_FIELDS or type(binding['schema']) is not int or binding['schema'] != 2 or len(canonical(binding)) > 4096:
        raise ValueError(MISMATCH)
    for field in ('message_request_id', 'machine_id', 'session_id'):
        if not canonical_uuid(binding[field]): raise ValueError(MISMATCH)
    for field, maximum, empty in [('computer_id',36,False), ('session',512,False), ('run_id',128,False), ('conversation_id',256,True)]:
        if not text(binding[field],maximum,empty): raise ValueError(MISMATCH)
    if binding['provider'] not in {'claude','codex','kimi'} or binding['native_encoding'] not in {'compact','python_default'} or not isinstance(binding['native_digest'],str) or not re.fullmatch('[0-9a-f]{64}',binding['native_digest']):
        raise ValueError(MISMATCH)


def validate(operation, wrapper, request_id, computer, session):
    required={'schema','request_id','binding','binding_sha256'} | ({'payload'} if operation==SEND else set())
    if operation not in OPERATIONS or not isinstance(wrapper,dict) or set(wrapper)!=required or type(wrapper['schema']) is not int or wrapper['schema']!=2 or wrapper['request_id']!=request_id or not canonical_uuid(request_id): raise ValueError(MISMATCH)
    b=wrapper['binding'];validate_binding(b)
    if b['computer_id']!=computer or b['session']!=session or digest(b)!=wrapper['binding_sha256']: raise ValueError(MISMATCH)
    if operation==SEND:
        from .attachments import validate_send
        p=wrapper['payload'];validate_send(p)
        if b['message_request_id']!=request_id or p.get('request_id')!=request_id or 'agent_id' in p or p.get('expected_run_id')!=b['run_id'] or p.get('expected_conversation_id')!=b['conversation_id'] or native_digest(p,b['native_encoding'])!=b['native_digest']: raise ValueError(MISMATCH)
    elif b['message_request_id']==request_id: raise ValueError(MISMATCH)


def negotiated(argv,payload=None):
    if len(argv)>=2 and argv[0]=="--expect-machine": argv=argv[2:]
    if len(argv)==3 and argv[0]=='swarm' and argv[1] in {'mobile-peer','__mobile-peer-local'} and argv[2]=='--json' and isinstance(payload,dict):
        argv=payload.get('argv',[])
    if len(argv)>=2 and argv[0]=='--expect-machine': argv=argv[2:]
    return (len(argv)==4 and argv[0]=='send' and argv[-2:]==['--delivery-outcome-v2','--json']) or (len(argv)==6 and argv[0]=='send' and argv[2]=='--session-id' and argv[-2:]==['--delivery-outcome-v2','--json'])


def negative(answer,read=False):
    fields=NEGATIVE_FIELDS | ({'message_request_id','agent_id'} if read else set())
    return (isinstance(answer,dict) and set(answer)==fields and type(answer.get('schema')) is int and answer['schema']==2 and answer.get('status')=='not_offered' and answer.get('proof')=='native_input_lock' and len(canonical(answer))<=8192 and text(answer.get('reason'),4096))


def query(wrapper):
    b=wrapper['binding']
    return {'schema':2,'request_id':wrapper['request_id'],'message_request_id':b['message_request_id'],
            'expected_machine_id':b['machine_id'],'expected_session_id':b['session_id'],'expected_run_id':b['run_id'],
            'expected_conversation_id':b['conversation_id'],'provider':b['provider'],'agent_id':'','native_digest':b['native_digest']}


def project(wrapper,answer,initial=False):
    b=wrapper['binding'];validate_binding(b)
    if not isinstance(answer,dict) or type(answer.get('schema')) is not int or answer['schema']!=2 or answer.get('request_id')!=wrapper['request_id'] or answer.get('name')!=b['session']: raise ValueError(MISMATCH)
    for field in ('machine_id','session_id','provider','run_id','conversation_id','native_digest'):
        if answer.get(field)!=b[field]: raise ValueError(MISMATCH)
    if initial:
        if answer['request_id']!=b['message_request_id']: raise ValueError(MISMATCH)
    elif answer.get('message_request_id')!=b['message_request_id'] or answer.get('agent_id')!='': raise ValueError(MISMATCH)
    result={'schema':2,'request_id':wrapper['request_id'],'binding':b,'binding_sha256':digest(b),'status':answer.get('status')}
    if answer.get('status')=='not_offered':
        if not negative(answer,not initial): raise ValueError(MISMATCH)
        result.update(proof='native_input_lock',reason=answer['reason'])
    elif answer.get('status')=='submitted' and not initial and answer.get('proof')=='original_receipt' and type(answer.get('submitted_at')) in (int,float) and math.isfinite(answer['submitted_at']) and answer['submitted_at']>=0 and type(answer.get('queued')) is bool:
        result.update(proof='original_receipt',submitted_at=answer['submitted_at'],queued=answer['queued'])
    elif answer.get('status')!='unknown' or initial: raise ValueError(MISMATCH)
    return result


def receipt_read(argv, payload=None):
    if len(argv)>=2 and argv[0]=="--expect-machine": argv=argv[2:]
    if len(argv)==3 and argv[:2] in (['swarm','mobile-peer'],['swarm','__mobile-peer-local']) and isinstance(payload,dict):
        argv,payload=payload.get('argv',[]),payload.get('payload')
    if len(argv)>=2 and argv[0]=='--expect-machine': argv=argv[2:]
    return len(argv)==5 and argv[:3]==['__state','delivery','receipt'] and argv[-1]=='--json' and isinstance(payload,dict) and payload.get('schema')==2


def closed_frame(answer):
    """Rust NativeProofFrame's key policy; project() checks values and identity."""
    return isinstance(answer,dict) and FRAME_REQUIRED <= answer.keys() <= FRAME_FIELDS


def unique_members(pairs):
    value={}
    for key,member in pairs:
        if key in value: raise ValueError('duplicate native proof field')
        value[key]=member
    return value
