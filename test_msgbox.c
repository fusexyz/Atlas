extern int MessageBoxA(void* hwnd, const char* text, const char* caption, int type);

int main() {
    MessageBoxA(0, "Hello from my compiler!", "My Compiler", 0);
    return 0;
}
