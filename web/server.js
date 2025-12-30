const http = require('http');
const fs = require('fs');
const path = require('path');
const { URL } = require('url');

const PORT = process.env.PORT ? parseInt(process.env.PORT, 10) : 8080;
// 许多受限环境不允许绑定 0.0.0.0（会触发 EPERM），默认仅监听本机。
const HOST = process.env.HOST || '127.0.0.1';
const WEB_ROOT = path.resolve(__dirname);

function safeResolveFromWebRoot(requestPathname) {
    let decodedPathname;
    try {
        decodedPathname = decodeURIComponent(requestPathname);
    } catch {
        return null;
    }

    // URL path 使用 POSIX 分隔符，规范化后去掉前导斜杠，避免 path.join 被当成绝对路径。
    const normalized = path.posix.normalize(decodedPathname).replace(/^\/+/, '');
    const resolved = path.resolve(WEB_ROOT, normalized);
    if (resolved === WEB_ROOT) return resolved;
    if (resolved.startsWith(WEB_ROOT + path.sep)) return resolved;
    return null;
}

// MIME 类型映射
const mimeTypes = {
    '.html': 'text/html',
    '.js': 'text/javascript',
    '.mjs': 'text/javascript',
    '.cjs': 'text/javascript',
    '.css': 'text/css',
    '.json': 'application/json',
    '.png': 'image/png',
    '.jpg': 'image/jpg',
    '.gif': 'image/gif',
    '.svg': 'image/svg+xml',
    '.wasm': 'application/wasm',
    '.map': 'application/json'
};

const server = http.createServer((req, res) => {
    console.log(`${new Date().toISOString()} - ${req.method} ${req.url}`);

    // 解析 URL（剔除 query/hash），并防止目录穿越
    const parsedUrl = new URL(req.url, `http://${req.headers.host || 'localhost'}`);
    const requestPathname = parsedUrl.pathname;
    const isDirectoryRequest = requestPathname.endsWith('/');
    const resolved = safeResolveFromWebRoot(requestPathname);
    if (!resolved) {
        res.writeHead(400, { 'Content-Type': 'text/plain' });
        res.end('Bad Request', 'utf-8');
        return;
    }

    const filePath = isDirectoryRequest ? path.join(resolved, 'index.html') : resolved;

    // 获取文件扩展名
    const extname = String(path.extname(filePath)).toLowerCase();
    const contentType = mimeTypes[extname] || 'application/octet-stream';

    // 读取文件
    fs.readFile(filePath, (error, content) => {
        if (error) {
            if (error.code === 'ENOENT') {
                // 文件不存在，返回 404
                res.writeHead(404, { 'Content-Type': 'text/html' });
                res.end(`
                    <h1>404 - 文件未找到</h1>
                    <p>请求的文件 ${requestPathname} 不存在</p>
                    <p><a href="/">返回首页</a></p>
                `, 'utf-8');
            } else {
                // 服务器错误
                res.writeHead(500);
                res.end(`服务器错误: ${error.code}`, 'utf-8');
            }
        } else {
            // 成功返回文件
            res.writeHead(200, {
                'Content-Type': contentType,
                'Cross-Origin-Opener-Policy': 'same-origin',
                'Cross-Origin-Embedder-Policy': 'require-corp',
                'Cache-Control': 'no-cache'
            });
            res.end(content);
        }
    });
});

server.listen(PORT, HOST, () => {
    console.log(`🚀 Alpha Finance 服务器启动成功！`);
    const localUrl = HOST === '0.0.0.0' ? `http://localhost:${PORT}` : `http://${HOST}:${PORT}`;
    console.log(`📍 访问地址: ${localUrl}`);
    if (HOST === '0.0.0.0') {
        console.log(`🌐 局域网访问: http://<你的IP>:${PORT}`);
    }
    console.log(`⏰ 启动时间: ${new Date().toLocaleString()}`);
    console.log(`\n📝 说明:`);
    console.log(`   - 首次运行需要先构建 WASM 模块`);
    console.log(`   - 推荐: ../start-web.sh（自动构建并启动）`);
    console.log(`   - 仅构建: ../build-wasm.sh（可选加 --serve 启动静态服务器）`);
    console.log(`\n按 Ctrl+C 停止服务器`);
});

// 优雅关闭
process.on('SIGINT', () => {
    console.log('\n👋 正在关闭服务器...');
    server.close(() => {
        console.log('✅ 服务器已关闭');
        process.exit(0);
    });
});
