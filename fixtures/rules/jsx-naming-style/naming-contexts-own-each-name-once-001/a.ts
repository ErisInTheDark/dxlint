
        const validName = {
            bad_property: 1,
            bad_method() {},
            goodProperty: 1,
        };
        class bad_class {
            bad_property = 1;
            bad_method(BadParameter: number) {}
        }
        enum GoodEnum { bad_member }
        function BadFunction() {}
    