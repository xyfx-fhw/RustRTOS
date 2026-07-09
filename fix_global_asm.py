import os
import re

def process_file(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    # Regex to find global_asm!( ... );
    # We'll use a relatively simple state machine or regex.
    # Actually, a regex that matches `global_asm!(` up to `);`
    pattern = re.compile(r'global_asm!\((.*?)\);', re.DOTALL)
    
    def replacer(match):
        inner = match.group(1)
        # Check if already using r#"
        if 'r#"' in inner:
            return match.group(0)
            
        lines = inner.split('\n')
        new_lines = []
        for line in inner.split('\n'):
            # Strip whitespace, quotes, and commas
            stripped = line.strip()
            if not stripped:
                continue
            if stripped.startswith('//'):
                new_lines.append("    " + stripped)
                continue
                
            # If it's a string, strip starting quote, ending quote, and comma
            if stripped.startswith('"'):
                s = stripped[1:]
                if s.endswith('",'):
                    s = s[:-2]
                elif s.endswith('"'):
                    s = s[:-1]
                # Unescape \" to "
                s = s.replace('\\"', '"')
                new_lines.append("    " + s)
            else:
                new_lines.append(line) # keep as is if not matching expectations
                
        return 'global_asm!(r#"\n' + '\n'.join(new_lines) + '\n"#);'

    new_content = pattern.sub(replacer, content)
    
    if new_content != content:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(new_content)
        print(f"Updated {filepath}")

for root, _, files in os.walk('.'):
    for file in files:
        if file.endswith('.rs') or file.endswith('.md'):
            process_file(os.path.join(root, file))

