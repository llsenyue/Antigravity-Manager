"""
Antigravity IDE 请求抓包脚本
使用 mitmproxy 捕获 IDE 发送到 cloudcode-pa.googleapis.com 的请求

使用方法:
1. pip install mitmproxy
2. mitmdump -s capture_antigravity.py -p 8888
3. 配置系统代理为 127.0.0.1:8888
4. 打开 Antigravity IDE 并使用 Claude 模型
5. 查看输出的请求详情
"""

import json
from mitmproxy import http
from datetime import datetime

# 目标 API 端点
TARGET_HOSTS = [
    "cloudcode-pa.googleapis.com",
    "daily-cloudcode-pa.sandbox.googleapis.com"
]

def request(flow: http.HTTPFlow) -> None:
    """捕获请求"""
    if any(host in flow.request.host for host in TARGET_HOSTS):
        print("\n" + "=" * 80)
        print(f"[{datetime.now().strftime('%H:%M:%S')}] 捕获请求: {flow.request.method} {flow.request.url}")
        print("=" * 80)
        
        # 打印所有 Headers
        print("\n📋 请求 Headers:")
        print("-" * 40)
        for key, value in flow.request.headers.items():
            # 隐藏敏感的 Authorization token
            if key.lower() == "authorization":
                value = value[:30] + "..." if len(value) > 30 else value
            print(f"  {key}: {value}")
        
        # 打印请求体
        if flow.request.content:
            print("\n📦 请求 Body:")
            print("-" * 40)
            try:
                body = json.loads(flow.request.content)
                # 只打印关键字段，避免 token 泄露
                safe_body = {
                    "model": body.get("model"),
                    "contents_count": len(body.get("contents", [])),
                    "tools": bool(body.get("tools")),
                    "generationConfig": body.get("generationConfig"),
                    "safetySettings": bool(body.get("safetySettings")),
                    "_raw_keys": list(body.keys())
                }
                print(json.dumps(safe_body, indent=2, ensure_ascii=False))
            except:
                print(f"  (binary data, {len(flow.request.content)} bytes)")
        
        print("\n")

def response(flow: http.HTTPFlow) -> None:
    """捕获响应"""
    if any(host in flow.request.host for host in TARGET_HOSTS):
        print(f"[{datetime.now().strftime('%H:%M:%S')}] 响应状态: {flow.response.status_code}")
        
        # 如果是错误响应，打印详情
        if flow.response.status_code >= 400:
            print(f"❌ 错误响应:")
            try:
                error = json.loads(flow.response.content)
                print(json.dumps(error, indent=2, ensure_ascii=False))
            except:
                print(flow.response.content[:500])
        print("\n")
