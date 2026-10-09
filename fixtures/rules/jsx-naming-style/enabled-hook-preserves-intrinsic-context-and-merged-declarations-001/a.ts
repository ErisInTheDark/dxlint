
        import bad_default from "module";
        import * as bad_namespace from "module";
        import { bad_named, source_name as bad_alias, default as bad_specifier } from "module";
        interface bad_merge {}
        class bad_merge {
            constructor() {}
            ["bad_computed"] = bad_default;
        }
        bad_named; bad_alias; bad_namespace; bad_specifier;
    