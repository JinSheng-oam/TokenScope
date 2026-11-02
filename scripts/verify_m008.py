#!/usr/bin/env python3
"""Independent SQLite/protobuf Antigravity fixture oracle.

Does NOT compile or run Rust. A passing result validates sample assumptions,
SQLite WAL read behavior and aggregation semantics only.
"""
from __future__ import annotations
import sqlite3
import tempfile
from collections import Counter
from pathlib import Path

def vint(n):
    b=[]
    while True:
        part=n&127;n>>=7
        b.append(part | (128 if n else 0))
        if not n: return bytes(b)

def field(number, data):
    return vint(number*8+2)+vint(len(data))+data

def gen_blob(model):
    return field(1, field(19, model.encode()))

def read_varint(blob, offset):
    val=0
    for shift in range(0,70,7):
        if offset >= len(blob): raise ValueError('truncated')
        byte=blob[offset];offset+=1
        if shift==63 and byte>1: raise ValueError('overflow')
        val|=(byte&127)<<shift
        if not byte & 128: return val,offset
    raise ValueError('overflow')

def field_value(data, number):
    pos=0
    while pos<len(data):
        tag,pos=read_varint(data,pos)
        if tag>>3==0: raise ValueError('zero tag')
        wire=tag&7
        if wire==0: _,pos=read_varint(data,pos)
        elif wire==1: pos+=8
        elif wire==5: pos+=4
        elif wire==2:
            size,pos=read_varint(data,pos)
            if pos+size>len(data):raise ValueError('truncated length')
            val=data[pos:pos+size];pos+=size
            if tag>>3==number: return val
        else: raise ValueError('unknown wire')
        if pos>len(data):raise ValueError('out of bounds')
    return None

def model_from_blob(b):
    nested=field_value(b,1)
    if nested is None:return None
    name=field_value(nested,19)
    if name is None:return None
    return name.decode()

def scan(path):
    connection=sqlite3.connect(f'file:{path}?mode=ro',uri=True)
    try:
        connection.execute('BEGIN')
        counts=Counter()
        for (blob,) in connection.execute('select data from gen_metadata order by rowid'):
            model=model_from_blob(blob)
            if model:counts[model]+=1
        return counts
    finally: connection.close()

def main():
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        paths=[]
        for n in ('antigravity','antigravity-cli'):
            p=root/n/'conversations'/'session-1.db'
            p.parent.mkdir(parents=True)
            conn=sqlite3.connect(p)
            conn.execute('create table gen_metadata (data blob not null)')
            conn.execute('pragma journal_mode=wal')
            for model in (('gemini-3-flash-a','gemini-3-flash-a') if n=='antigravity' else ('gemini-3-flash-a',)):
                conn.execute('insert into gen_metadata values (?)',(gen_blob(model),))
            conn.commit();conn.close();paths.append(p)
        sources=[scan(p) for p in paths]
        assert sources[0]['gemini-3-flash-a']==2
        assert sources[1]['gemini-3-flash-a']==1
        # Superset selection, not addition across a synced conversation.
        selected=max(sources,key=lambda x:sum(x.values()))
        assert selected==Counter({'gemini-3-flash-a':2})
        estimate_input=selected['gemini-3-flash-a']*4000
        estimate_output=selected['gemini-3-flash-a']*500
        assert (estimate_input,estimate_output)==(8000,1000)
        assert model_from_blob(gen_blob('claude-sonnet-4'))=='claude-sonnet-4'
        try: field_value(b'\x0a\xff',1)
        except ValueError: pass
        else: raise AssertionError('truncated protobuf accepted')
        print('M0-08 independent SQLite/protobuf oracle PASS (Rust not executed)')
if __name__=='__main__': main()
