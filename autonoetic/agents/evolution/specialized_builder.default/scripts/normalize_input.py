import sys
import json

def normalize_input(input_str):
    """
    Normalizes input that might be plain text into the expected
    JSON object for the specialized builder.
    """
    try:
        # Check if it's already a JSON object
        data = json.loads(input_str)
        if isinstance(data, dict):
            # If it's a dict, check if it has the required fields.
            # If not, we might still want to wrap it, but for now
            # let's assume if it's JSON, it's intended to be structured.
            return input_str
    except json.JSONDecodeError:
        pass

    # If it's not JSON, wrap it as a requirements message
    normalized = {
        "role": "specialist",
        "description": input_str.strip(),
        "requirements": [input_str.strip()],
        "constraints": []
    }
    return json.dumps(normalized)

if __name__ == "__main__":
    # Autonoetic calls pre-process scripts with the input as the only argument
    # or via stdin. The gateway implementation shows it pipes input to the script.
    
    input_text = sys.stdin.read()
    if not input_text:
        sys.exit(0)
        
    print(normalize_input(input_text))
