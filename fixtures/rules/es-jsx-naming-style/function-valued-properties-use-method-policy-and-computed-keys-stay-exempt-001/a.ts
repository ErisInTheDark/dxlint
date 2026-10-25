
        class Example {
            GoodMethod = () => 1;
            GOOD_PROPERTY = 1;
            ["computed_key"] = 1;
            badMethod = () => 1;
        }
        interface ExampleType {
            GoodMethod: () => number;
            GOOD_PROPERTY: number;
            badMethod: () => number;
        }
    