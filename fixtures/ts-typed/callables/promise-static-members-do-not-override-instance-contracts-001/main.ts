
        class Base { method(): void {} field = (): void => {}; accessor callback = (): void => {} }
        class Static extends Base { static async method() {} static field = async () => {}; static accessor callback = async () => {} }
        class Instance extends Base { async method() {} field = async () => {}; accessor callback = async () => {} }
    