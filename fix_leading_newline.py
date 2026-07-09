import os
import re

def fix_empty_lines(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    # 将 `global_asm!(r#"` 后面的多个连续换行替换为单个换行
    new_content = re.sub(r'global_asm!\(r#"\n+', 'global_asm!(r#"\n', content)
    
    # 顺便把结尾 `"#);` 前面多余的空行也清理一下，避免尾部留白
    new_content = re.sub(r'\n+"#\);', '\n"#);', new_content)

    if new_content != content:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(new_content)
        print(f"Fixed leading/trailing newlines in {filepath}")

for root, _, files in os.walk('.'):
    for file in files:
        if file.endswith('.rs') or file.endswith('.md'):
            fix_empty_lines(os.path.join(root, file))

