"""
Rust Crawler Web Server
提供 REST API 来启动爬虫并获取结果
"""
import subprocess
import json
import os
import threading
import time
import uuid
import csv
import sqlite3
from pathlib import Path
from flask import Flask, request, jsonify, send_from_directory
from flask_cors import CORS

app = Flask(__name__, static_folder='.')
CORS(app)

# 任务存储
tasks = {}
# 爬虫二进制路径
CRAWLER_BIN = str(Path(__file__).parent / 'target' / 'release' / 'rust-crawler.exe')
CRAWLER_BIN_DEBUG = str(Path(__file__).parent / 'target' / 'debug' / 'rust-crawler.exe')

if not os.path.exists(CRAWLER_BIN):
    CRAWLER_BIN = CRAWLER_BIN_DEBUG

OUTPUT_DIR = Path(__file__).parent / 'crawl_outputs'
OUTPUT_DIR.mkdir(exist_ok=True)


def run_crawler(task_id: str, params: dict):
    """在后台线程中运行爬虫"""
    try:
        tasks[task_id]['status'] = 'running'
        tasks[task_id]['logs'] = []

        cmd = [CRAWLER_BIN]

        # 种子 URL（必选）
        seeds = params.get('seeds', [])
        for s in seeds:
            cmd.extend(['--seed', s.strip()])

        if not seeds:
            raise ValueError('至少需要一个种子 URL')

        # 可选参数
        int_params = {
            'max-depth': 'max_depth',
            'max-concurrency': 'max_concurrency',
            'timeout-secs': 'timeout_secs',
            'retry-count': 'retry_count',
            'retry-delay-ms': 'retry_delay_ms',
            'delay-ms': 'delay_ms',
            'max-pages': 'max_pages',
        }
        for flag, key in int_params.items():
            val = params.get(key)
            if val is not None and val != '':
                cmd.extend([f'--{flag}', str(val)])

        # 域名白名单
        allowed = params.get('allowed_domains', '')
        if allowed:
            for d in allowed.split(','):
                d = d.strip()
                if d:
                    cmd.extend(['--allow', d])

        # 排除模式
        excluded = params.get('exclude_patterns', '')
        if excluded:
            for p in excluded.split(','):
                p = p.strip()
                if p:
                    cmd.extend(['--exclude', p])

        # 包含模式
        includes = params.get('include_patterns', '')
        if includes:
            for p in includes.split(','):
                p = p.strip()
                if p:
                    cmd.extend(['--include', p])

        # 分页参数
        page_template = params.get('page_template', '')
        if page_template:
            cmd.extend(['--page-template', page_template])
        page_range = params.get('page_range', '')
        if page_range:
            cmd.extend(['--page-range', page_range])

        # User-Agent
        ua = params.get('user_agent', '')
        if ua:
            cmd.extend(['--user-agent', ua])

        # 输出格式
        fmt = params.get('format', 'json')
        # 内部始终用 JSON（前端需要 JSON 展示）
        api_fmt = 'json'
        cmd.extend(['--format', api_fmt])

        # 输出路径（JSON 格式用于 API 展示）
        json_file = OUTPUT_DIR / f'crawl_{task_id}'
        cmd.extend(['--output', str(json_file)])

        # robots.txt
        if params.get('respect_robots'):
            cmd.append('--respect-robots')

        # 详细日志
        if params.get('verbose'):
            cmd.append('--verbose')

        tasks[task_id]['cmd'] = ' '.join(cmd)
        tasks[task_id]['logs'].append(f'🚀 启动命令: {" ".join(cmd)}')

        # 运行爬虫（逐行读取，实时更新日志）
        start_time = time.time()
        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=False,
        )

        # 逐行读取 stdout
        stdout_lines = []
        for raw_line in process.stdout:
            line = raw_line.decode('utf-8', errors='replace').rstrip('\n\r')
            if line:
                stdout_lines.append(line)
                tasks[task_id]['logs'].append(line)

        # 读取 stderr
        stderr_data = process.stderr.read()
        if stderr_data:
            for line in stderr_data.decode('utf-8', errors='replace').split('\n'):
                line = line.strip()
                if line:
                    tasks[task_id]['logs'].append(line)

        process.wait(timeout=600)
        elapsed = time.time() - start_time
        stdout = '\n'.join(stdout_lines)

        # 解析结果
        json_path = json_file.with_suffix('.json')
        if json_path.exists():
            with open(json_path, 'r', encoding='utf-8') as f:
                content = f.read()
            try:
                data = json.loads(content)
                tasks[task_id]['result'] = data
                tasks[task_id]['stats'] = {
                    'total': len(data),
                    'success': sum(1 for p in data if 200 <= p.get('status_code', 0) < 400),
                    'failed': sum(1 for p in data if not (200 <= p.get('status_code', 0) < 400)),
                    'total_links': sum(len(p.get('links', [])) for p in data),
                    'max_depth': max((p.get('depth', 0) for p in data), default=0),
                    'elapsed_sec': round(elapsed, 2),
                }
                tasks[task_id]['logs'].append(f'📊 抓取 {len(data)} 页，成功 {tasks[task_id]["stats"]["success"]} 页，失败 {tasks[task_id]["stats"]["failed"]} 页')
                tasks[task_id]['logs'].append(f'🔗 发现链接: {tasks[task_id]["stats"]["total_links"]}')

                # 如果用户选的是 CSV/SQLite，额外生成该格式文件
                if fmt != 'json':
                    output_path = json_file.with_suffix(f'.{fmt}')
                    if fmt == 'csv':
                        with open(output_path, 'w', newline='', encoding='utf-8-sig') as f:
                            if data:
                                writer = csv.DictWriter(f, fieldnames=data[0].keys())
                                writer.writeheader()
                                writer.writerows(data)
                        tasks[task_id]['logs'].append(f'💾 CSV 文件已保存: {output_path.name}')
                    elif fmt == 'sqlite':
                        db_path = output_path
                        if os.path.exists(db_path):
                            os.remove(db_path)
                        conn = sqlite3.connect(str(db_path))
                        c = conn.cursor()
                        c.execute('CREATE TABLE IF NOT EXISTS pages (url TEXT, title TEXT, content TEXT, status_code INT, depth INT, fetch_duration_ms INT, crawled_at INT)')
                        c.execute('CREATE TABLE IF NOT EXISTS links (page_url TEXT, link_text TEXT, link_url TEXT, is_internal INT)')
                        for p in data:
                            c.execute('INSERT OR REPLACE INTO pages VALUES (?,?,?,?,?,?,?)',
                                      (p.get('url'), p.get('title'), p.get('content'),
                                       p.get('status_code'), p.get('depth'),
                                       p.get('fetch_duration_ms'), p.get('crawled_at')))
                            for l in p.get('links', []):
                                c.execute('INSERT INTO links VALUES (?,?,?,?)',
                                          (p['url'], l.get('text'), l.get('url'), 1 if l.get('is_internal') else 0))
                        conn.commit()
                        conn.close()
                        tasks[task_id]['logs'].append(f'💾 SQLite 文件已保存: {output_path.name}')
            except json.JSONDecodeError:
                pass
            tasks[task_id]['logs'].append(f'✅ 爬取完成！耗时 {elapsed:.1f} 秒')
            tasks[task_id]['status'] = 'done'
        else:
            tasks[task_id]['status'] = 'error'
            tasks[task_id]['logs'].append('❌ 未找到输出文件')
            # 尝试输出目录下找文件
            for f in OUTPUT_DIR.glob(f'crawl_{task_id}*'):
                tasks[task_id]['logs'].append(f'  找到输出: {f.name}')

    except subprocess.TimeoutExpired:
        tasks[task_id]['status'] = 'error'
        tasks[task_id]['logs'].append('❌ 爬虫超时（超过 10 分钟）')
        try:
            process.kill()
        except:
            pass
    except Exception as e:
        tasks[task_id]['status'] = 'error'
        tasks[task_id]['logs'].append(f'❌ 错误: {str(e)}')


@app.route('/')
def index():
    return send_from_directory('.', 'crawler-ui.html')


@app.route('/api/crawl', methods=['POST'])
def start_crawl():
    """启动爬虫任务"""
    params = request.json or {}
    task_id = str(uuid.uuid4())[:8]

    tasks[task_id] = {
        'id': task_id,
        'status': 'queued',
        'params': params,
        'logs': [],
        'result': None,
        'stats': None,
        'cmd': '',
    }

    # 后台启动爬虫
    thread = threading.Thread(target=run_crawler, args=(task_id, params), daemon=True)
    thread.start()

    return jsonify({'task_id': task_id, 'status': 'queued'})


@app.route('/api/crawl/<task_id>', methods=['GET'])
def get_status(task_id):
    """查询任务状态和结果"""
    task = tasks.get(task_id)
    if not task:
        return jsonify({'error': '任务不存在'}), 404

    return jsonify({
        'task_id': task_id,
        'status': task['status'],
        'logs': task.get('logs', []),
        'result': task.get('result'),
        'stats': task.get('stats'),
        'cmd': task.get('cmd', ''),
        'params': task.get('params', {}),
    })


@app.route('/api/tasks', methods=['GET'])
def list_tasks():
    """列出所有任务"""
    return jsonify([
        {'id': t['id'], 'status': t['status'], 'params': t.get('params', {}),
         'stats': t.get('stats')}
        for t in tasks.values()
    ])


if __name__ == '__main__':
    print('=== Rust Crawler Web Server ===')
    print(f'Crawler binary: {CRAWLER_BIN}')
    print(f'Output dir: {OUTPUT_DIR}')
    print(f'Open browser at: http://127.0.0.1:5000')
    print()
    app.run(host='127.0.0.1', port=5000, debug=False, threaded=True)
