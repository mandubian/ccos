#!/bin/bash

# Download a recommended model for RTFS local inference
# This script downloads Microsoft Phi-2, which is efficient and performs well

MODEL_DIR="models"
MODEL_NAME="phi-2.Q4_K_M.gguf"
MODEL_URL="https://huggingface.co/TheBloke/phi-2-GGUF/resolve/main/phi-2.Q4_K_M.gguf"
MODEL_PATH="$MODEL_DIR/$MODEL_NAME"

echo "Downloading Microsoft Phi-2 model for RTFS local inference..."
echo "Model: $MODEL_NAME"
echo "Size: ~1.5GB"
echo ""

# Create models directory if it doesn't exist
mkdir -p "$MODEL_DIR"

# Check if model already exists
if [ -f "$MODEL_PATH" ]; then
    echo "Model already exists at $MODEL_PATH"
    echo "To use it, set the environment variable:"
    echo "export RTFS_LOCAL_MODEL_PATH=$MODEL_PATH"
    exit 0
fi

# Download the model
echo "Downloading model from $MODEL_URL..."
echo "This may take a few minutes depending on your internet connection..."
echo ""

if command -v wget &> /dev/null; then
    wget -O "$MODEL_PATH" "$MODEL_URL"
elif command -v curl &> /dev/null; then
    curl -L -o "$MODEL_PATH" "$MODEL_URL"
else
    echo "Error: Neither wget nor curl found. Please install one of them."
    exit 1
fi

# Check if download was successful
if [ -f "$MODEL_PATH" ]; then
    echo ""
    echo "✅ Model downloaded successfully!"
    echo "Model saved to: $MODEL_PATH"
    echo ""
    echo "To use this model with RTFS, set the environment variable:"
    echo "export RTFS_LOCAL_MODEL_PATH=$MODEL_PATH"
    echo ""
    echo "Or add it to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo "echo 'export RTFS_LOCAL_MODEL_PATH=$MODEL_PATH' >> ~/.bashrc"
    echo ""
    echo "Alternative models you can try:"
    echo "  - Llama-2-7B-Chat: https://huggingface.co/TheBloke/Llama-2-7B-Chat-GGUF"
    echo "  - Mistral-7B-Instruct: https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.2-GGUF"
    echo "  - CodeLlama-7B: https://huggingface.co/TheBloke/CodeLlama-7B-Python-GGUF"
else
    echo "❌ Failed to download model"
    exit 1
fi 