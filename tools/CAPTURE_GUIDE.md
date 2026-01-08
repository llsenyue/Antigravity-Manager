# 如何抓包 Antigravity IDE 的请求

## 方法一：Fiddler Classic（推荐）

### 步骤

1. 下载安装 Fiddler Classic: <https://www.telerik.com/download/fiddler>
2. 打开 Fiddler，进入 Tools → Options → HTTPS
3. 勾选 "Capture HTTPS CONNECTs" 和 "Decrypt HTTPS traffic"
4. 点击 "Actions" → "Trust Root Certificate" 安装 CA 证书
5. 在 Filters 标签页设置只捕获目标域名：
   - 勾选 "Use Filters"
   - Host 设置为：cloudcode-pa.googleapis.com
6. 打开 Antigravity IDE，使用 Claude 模型
7. 查看 Fiddler 中捕获的请求

### 重点关注的 Headers

- User-Agent
- X-Goog-Api-Client
- X-Goog-Api-Key
- X-Client-Version
- X-ClientDetails
- 任何以 X- 开头的自定义 header

---

## 方法二：mitmproxy（命令行）

### 安装

```powershell
pip install mitmproxy
```

### 启动代理

```powershell
mitmdump -p 8888 --ssl-insecure
```

### 配置 Windows 系统代理

1. 打开 Windows 设置 → 网络和 Internet → 代理
2. 手动设置代理：
   - 地址：127.0.0.1
   - 端口：8888
3. 保存

### 安装 CA 证书

1. 打开浏览器访问 <http://mitm.it>
2. 下载 Windows 证书并安装到"受信任的根证书颁发机构"

### 使用抓包脚本

```powershell
mitmdump -s f:\backup\home\llsenyue\project\Antigravity-Manager-Doctor\tools\capture_antigravity.py -p 8888
```

---

## 方法三：Chrome DevTools（如果 IDE 基于 Electron）

如果 Antigravity IDE 是基于 Electron 的：

1. 右键点击 IDE 窗口 → 检查 (Inspect)
2. 切换到 Network 标签
3. 使用 Claude 模型
4. 查看请求详情

---

## 抓包后对比项目

| 项目 | Antigravity IDE | 反代理 |
|------|-----------------|--------|
| User-Agent | ? | antigravity/1.11.9 windows/amd64 |
| X-Goog-Api-Client | ? | gl-node/20.18.0 grpc/1.67.0 |
| X-Client-Version | ? | antigravity/1.11.9 |
| 其他 X- headers | ? | 无 |
| 请求 Body 结构 | ? | 需对比 |
