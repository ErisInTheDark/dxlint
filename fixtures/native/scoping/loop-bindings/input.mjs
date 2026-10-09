let first = 0, second = 0; while (first < 3) { queue(() => first + second + first); first++; second++; } queue(() => first);
