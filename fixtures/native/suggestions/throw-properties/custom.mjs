try {} catch (error) { throw new CustomError('text', {details: error, cause: error}); }
