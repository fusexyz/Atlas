int add(int a, int b) {
    return a + b;
}

int main() {
    int i = 0;
    int sum = 0;
    while (i < 10) {
        sum = sum + i;
        i++;
    }
    return add(sum, 1);
}
