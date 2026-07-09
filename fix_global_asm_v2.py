import os
import re
import subprocess

def get_head_version(filepath):
    try:
        # Fetch the file content from HEAD
        result = subprocess.run(['git', 'show', f'HEAD:{filepath}'], capture_output=True, text=True, check=True)
        return result.stdout
    except subprocess.CalledProcessError:
        return None

def process_file_from_head(filepath):
    # Read the current file (which has stripped empty lines and other possible edits)
    with open(filepath, 'r', encoding='utf-8') as f:
        current_content = f.read()

    # Get HEAD content
    head_content = get_head_version(filepath)
    if not head_content:
        return

    pattern = re.compile(r'global_asm!\((.*?)\);', re.DOTALL)
    
    # Extract original blocks from HEAD
    head_matches = list(pattern.finditer(head_content))
    # Extract current blocks from current file
    current_matches = list(pattern.finditer(current_content))
    
    if len(head_matches) != len(current_matches):
        print(f"Warning: match count mismatch in {filepath}")
        return
        
    new_content = current_content
    offset = 0
    
    for head_match, current_match in zip(head_matches, current_matches):
        head_inner = head_match.group(1)
        
        # process head_inner properly
        lines = head_inner.split('\n')
        new_lines = []
        for line in lines:
            stripped = line.strip()
            
            # Keep empty lines!
            if not stripped or stripped == '""' or stripped == '"",':
                new_lines.append("")
                continue
                
            if stripped.startswith('//'):
                new_lines.append("    " + stripped)
                continue
                
            if stripped.startswith('"'):
                s = stripped[1:]
                if s.endswith('",'):
                    s = s[:-2]
                elif s.endswith('"'):
                    s = s[:-1]
                s = s.replace('\\"', '"')
                new_lines.append("    " + s)
            else:
                new_lines.append(line)
        
        new_block = 'global_asm!(r#"\n' + '\n'.join(new_lines) + '\n"#);'
        
        # Replace the current block with the properly processed new block
        start = current_match.start() + offset
        end = current_match.end() + offset
        
        new_content = new_content[:start] + new_block + new_content[end:]
        offset += len(new_block) - (end - start)

    if new_content != current_content:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(new_content)
        print(f"Restored and Fixed newlines in {filepath}")

for root, _, files in os.walk('.'):
    for file in files:
        if file.endswith('.rs') or file.endswith('.md'):
            process_file_from_head(os.path.join(root, file))

