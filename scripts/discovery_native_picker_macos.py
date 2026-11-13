#!/usr/bin/env python3
"""Press a real OS picker button in the specified test process; never grant access."""
import ctypes as c
import json
import os
import time
ax=c.CDLL('/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices')
cf=c.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
ref=c.c_void_p
ax.AXIsProcessTrusted.restype=c.c_bool
ax.AXUIElementCreateApplication.argtypes=[c.c_int];ax.AXUIElementCreateApplication.restype=ref
ax.AXUIElementCopyAttributeValue.argtypes=[ref,ref,c.POINTER(ref)];ax.AXUIElementCopyAttributeValue.restype=c.c_int
ax.AXUIElementPerformAction.argtypes=[ref,ref];ax.AXUIElementPerformAction.restype=c.c_int
ax.AXUIElementSetMessagingTimeout.argtypes=[ref,c.c_float]
cf.CFStringCreateWithCString.argtypes=[ref,c.c_char_p,c.c_uint];cf.CFStringCreateWithCString.restype=ref
cf.CFRelease.argtypes=[ref]
cf.CFArrayGetCount.argtypes=[ref];cf.CFArrayGetCount.restype=c.c_long
cf.CFArrayGetValueAtIndex.argtypes=[ref,c.c_long];cf.CFArrayGetValueAtIndex.restype=ref
cf.CFGetTypeID.argtypes=[ref];cf.CFGetTypeID.restype=c.c_ulong
cf.CFArrayGetTypeID.restype=c.c_ulong
cf.CFStringGetTypeID.restype=c.c_ulong
cf.CFStringGetCString.argtypes=[ref,c.c_char_p,c.c_long,c.c_uint];cf.CFStringGetCString.restype=c.c_bool

def string(value):return cf.CFStringCreateWithCString(None,value.encode(),0x08000100)
def attribute(element,key):
 key=string(key);value=ref()
 try:code=ax.AXUIElementCopyAttributeValue(element,key,c.byref(value))
 finally:cf.CFRelease(key)
 return value.value if code==0 else None

def text(element,key):
 value=attribute(element,key)
 if not value:return None
 try:
  if cf.CFGetTypeID(value)!=cf.CFStringGetTypeID():return None
  buffer=c.create_string_buffer(512)
  return buffer.value.decode() if cf.CFStringGetCString(value,buffer,len(buffer),0x08000100) else None
 finally:cf.CFRelease(value)

def press(element,identifier,remaining):
 if remaining[0]<=0:return False
 remaining[0]-=1
 if text(element,'AXIdentifier')==identifier:
  action=string('AXPress')
  try:code=ax.AXUIElementPerformAction(element,action)
  finally:cf.CFRelease(action)
  if code:raise RuntimeError(f'Native button press failed: {code}')
  return True
 for key in ['AXWindows','AXSheets','AXChildren']:
  children=attribute(element,key)
  if not children:continue
  try:
   if cf.CFGetTypeID(children)!=cf.CFArrayGetTypeID():continue
   for index in range(cf.CFArrayGetCount(children)):
    if press(cf.CFArrayGetValueAtIndex(children,index),identifier,remaining):return True
  finally:cf.CFRelease(children)
 return False

if not ax.AXIsProcessTrusted():
 raise SystemExit('Existing Accessibility access is unavailable for native dialog automation; no permission was requested or changed.')
pid=int(os.environ['DEVFORGE_PICKER_PID']);action=os.environ['DEVFORGE_PICKER_ACTION']
assert action in ['select','cancel']
app=ax.AXUIElementCreateApplication(pid);ax.AXUIElementSetMessagingTimeout(app,1.0)
try:
 deadline=time.monotonic()+20
 while time.monotonic()<deadline:
  if press(app,'OKButton' if action=='select' else 'CancelButton',[2048]):
   print(json.dumps({'native_dialog':True,'pid_matched':True,'native_ax_press':True,'action':action,'existing_accessibility_permission':True}));break
  time.sleep(.1)
 else:raise RuntimeError('The test process native picker button did not appear')
finally:cf.CFRelease(app)
